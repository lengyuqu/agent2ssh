#!/usr/bin/env python3
"""Exercise private Mach endpoints in a temporary launchd-owned real daemon.

No user credentials or production daemon are used. Any launchd job with
uncertain cleanup leaves both its service label and temporary files for review.
"""
from pathlib import Path
import argparse
import json
import os
import plistlib
import pwd
import re
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT=Path(__file__).resolve().parent;REPO=ROOT.parents[3];DOMAIN=f'gui/{os.getuid()}'
TOKEN='a2s-mach-lab-synthetic-token'
parser=argparse.ArgumentParser();parser.add_argument('--daemon',type=Path,required=True);parser.add_argument('--cli',type=Path,default=REPO/'src-tauri/target/debug/agent2ssh');parser.add_argument('--archive',action='store_true');args=parser.parse_args()
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
env['A2S_MACH_LAB_EXPECT_PRIVATE']='1'
def run(args,timeout=12):return subprocess.run([str(x) for x in args],capture_output=True,text=True,timeout=timeout,env=env)
def must(args,timeout=12):
 p=run(args,timeout)
 if p.returncode:raise RuntimeError(f'{args}: {p.returncode}: {p.stdout} {p.stderr}')
 return p
def state(label):
 try:r=run(['launchctl','print',f'{DOMAIN}/{label}'])
 except (OSError,subprocess.TimeoutExpired):return 'unknown'
 if r.returncode==0:return 'present'
 if r.returncode==113 and 'Could not find service' in (r.stdout+r.stderr):return 'absent'
 return 'unknown'
def wait_for(predicate,seconds=15):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  value=predicate()
  if value:return value
  time.sleep(.03)
 raise TimeoutError('test condition not observed')
def alive(pid):
 p=run(['ps','-o','stat=','-p',str(pid)])
 return bool(p.stdout.strip()) and not p.stdout.strip().startswith('Z')
def free_port():
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));return sock.getsockname()[1]
def identity(config,port):
 try:
  record=json.loads((config/'daemon.instance.json').read_text());pid=record['pid']
  opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
  with opener.open(f'http://127.0.0.1:{port}/health',timeout=.3) as response:
   if json.load(response).get('pid')!=pid:return None
  return record
 except (OSError,ValueError,KeyError,urllib.error.URLError):return None

root=Path(tempfile.mkdtemp(prefix='a2s-launchd-daemon-',dir='/tmp'));configs=[];keep=False
try:
 binary=root/'daemon';client=root/'client';probe=root/'lookup';shutil.copy2(args.daemon.resolve(strict=True),binary)
 must(['codesign','--force','--sign','-','--options','runtime','--identifier','org.agent2ssh.research.launchddaemon',binary])
 signature=must(['codesign','-dv','--verbose=4',binary]).stderr;cdhash=re.search(r'^CDHash=([0-9a-f]{40})$',signature,re.MULTILINE)
 if not cdhash or 'Signature=adhoc' not in signature:raise AssertionError('expected ad-hoc signed test binary')
 requirement=f'identifier "org.agent2ssh.research.launchddaemon" and cdhash H"{cdhash.group(1)}"'
 must(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',f'-I{REPO / "src-tauri/src/bin/agent2ssh_daemon/mach_lab"}',ROOT/'client.c','-framework','Security','-framework','CoreFoundation','-lbsm','-o',client])
 must(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',REPO/'docs/reports/evidence/macos-mach-discovery-2026-11-13/bootstrap_probe.c','-o',probe])
 results=[]
 for i in range(1,4):
  config=Path(tempfile.mkdtemp(prefix='a2s-mach-lab-',dir='/tmp'));configs.append(config)
  (config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
  service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex[:12]}';plist=root/f'{i}.plist';ready=root/f'{i}.ready';port=free_port()
  variables={'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{port}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service,'A2S_MACH_LAB_LAUNCHD_DISCOVERY':'1'}
  plist.write_bytes(plistlib.dumps({'Label':service,'ProgramArguments':[str(binary)],'RunAtLoad':True,'MachServices':{service:True},'EnvironmentVariables':variables,'StandardOutPath':str(root/f'{i}.out'),'StandardErrorPath':str(root/f'{i}.err')}))
  installed=False;held=None;sshd=None;ssh_request=None
  try:
   if i==1:
    key=config/'key';hostkey=config/'hostkey';authorized=config/'authorized_keys';sshport=free_port()
    for path in (key,hostkey):must(['ssh-keygen','-q','-t','ed25519','-N','','-f',path])
    authorized.write_bytes(key.with_suffix('.pub').read_bytes());authorized.chmod(0o600)
    sshd_conf=config/'sshd_config';sshd_conf.write_text(f'Port {sshport}\nListenAddress 127.0.0.1\nHostKey {hostkey}\nPidFile {config}/sshd.pid\nAuthorizedKeysFile {authorized}\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nPermitUserRC no\nSetEnv HOME={config} ZDOTDIR={config}\n')
    user=pwd.getpwuid(os.getuid()).pw_name
    (config/'hosts.json').write_text(json.dumps({'schema_version':1,'hosts':[{'name':'fixture','host':'127.0.0.1','port':sshport,'user':user,'key_path':str(key)}]}))
    sshd=subprocess.Popen(['/usr/sbin/sshd','-D','-e','-f',str(sshd_conf)],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,env=env)
    time.sleep(.4)
    probe_home=must(['ssh','-F','/dev/null','-o','StrictHostKeyChecking=no','-o','UserKnownHostsFile=/dev/null','-o','IdentitiesOnly=yes','-o','BatchMode=yes','-i',key,'-p',str(sshport),f'{user}@127.0.0.1','printf "%s\\n%s\\n" "$HOME" "$ZDOTDIR"'])
    if probe_home.stdout.splitlines()!=[str(config),str(config)]:raise AssertionError('remote shell HOME was not isolated')
   must(['launchctl','bootstrap',DOMAIN,plist]);installed=True
   a=wait_for(lambda:identity(config,port));pid_a=a['pid']
   wrong=run([client,service,str(pid_a),a['instance_id'],'identifier "org.agent2ssh.wrong"','stop'])
   if wrong.returncode==0 or 'refused=code_or_task token_sent=0' not in wrong.stdout or not alive(pid_a):raise AssertionError(f'wrong code did not fail closed: {wrong.stdout} {wrong.stderr}')
   apple=run([client,service,str(pid_a),a['instance_id'],'anchor apple generic','stop'])
   if apple.returncode==0 or 'refused=code_or_task token_sent=0' not in apple.stdout or not alive(pid_a):raise AssertionError(f'ad-hoc code satisfied Apple anchor: {apple.stdout} {apple.stderr}')
   mismatched=run([client,service,str(pid_a),'f'*32,requirement,'stop'])
   if mismatched.returncode==0 or 'token_sent=0' not in mismatched.stdout or not alive(pid_a):raise AssertionError(f'wrong instance did not fail closed: {mismatched.stdout} {mismatched.stderr}')
   held_env=dict(env);held_env['A2S_MACH_LAB_CLIENT_READY']=str(ready)
   held=subprocess.Popen([str(client),service,str(pid_a),a['instance_id'],requirement,'hold'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=held_env)
   wait_for(lambda:ready.exists() or held.poll() is not None,seconds=8)
   if not ready.exists():raise AssertionError(f'held identity proof failed: {held.communicate()}')
   nonce=ready.read_text().strip()
   if not re.fullmatch(r'[0-9a-f]{32}',nonce):raise AssertionError('invalid held-client nonce')
   attack_env=dict(env);attack_env['A2S_MACH_LAB_TEST_NONCE']=nonce
   attack=subprocess.run([str(client),service,str(pid_a),a['instance_id'],requirement,'borrowed-stop'],capture_output=True,text=True,timeout=8,env=attack_env)
   if attack.returncode==0 or 'result=unknown' not in attack.stdout or not alive(pid_a):raise AssertionError(f'borrowed nonce was not rejected: {attack.stdout} {attack.stderr}')
   wait_for(lambda:(root/f'{i}.err').exists() and 'A2S_MACH_LAB_STOP_RECEIVED' in (root/f'{i}.err').read_text())
   public=run([client,service,str(pid_a),a['instance_id'],requirement,'public-stop'],timeout=8)
   if public.returncode==0 or 'result=unknown' not in public.stdout or not alive(pid_a):raise AssertionError(f'public stop was not rejected: {public.stdout} {public.stderr}')
   wait_for(lambda:(root/f'{i}.err').exists() and 'A2S_MACH_LAB_PUBLIC_STOP_REJECTED' in (root/f'{i}.err').read_text())
   if i==1:
    marker=config/'remote-ready';command=f"printf ready > '{marker}'; sleep 8"
    body=json.dumps({'host':'fixture','command':command,'force':True,'timeout_secs':20,'reason':'launchd test','side_effect':'synthetic','change_id':'launchd-ssh'}).encode()
    ssh_request=socket.create_connection(('127.0.0.1',port),timeout=3)
    ssh_request.sendall((f'POST /exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n').encode()+body)
    wait_for(marker.exists,seconds=12)
   stopped=must([client,service,str(pid_a),a['instance_id'],requirement,'wait-exit'],timeout=8)
   if 'result=accepted_exit_unverified' not in stopped.stdout or 'post_registration_identity_match=1' not in stopped.stdout or 'proc_exit_event=1 event_pid_matches=1 note_exit=1' not in stopped.stdout:raise AssertionError('first stop or exit observation failed')
   wait_for(lambda:not alive(pid_a) and not (config/'daemon.pid').exists())
   if i==1:
    ssh_request.settimeout(5);response=bytearray()
    while True:
     chunk=ssh_request.recv(4096)
     if not chunk:break
     response.extend(chunk)
    ssh_request.close();ssh_request=None
    if not response.startswith(b'HTTP/1.1 503') or b'remote command outcome unknown' not in response:raise AssertionError(f'wrong SSH drain response: {response!r}')
    audit=[json.loads(line) for line in (config/'audit.jsonl').read_text().splitlines()]
    entry=next((item for item in audit if item.get('change_id')=='launchd-ssh'),None)
    if not entry or 'remote command outcome unknown' not in entry.get('reason','') or entry.get('side_effect')!='synthetic':raise AssertionError('missing SSH drain audit')
   must([probe,'lookup',service,'send']) # synthetic public discovery wake-up
   b=wait_for(lambda:(v if (v:=identity(config,port)) and v['pid']!=pid_a else None),seconds=25);pid_b=b['pid']
   old_out,old_err=held.communicate(input='x',timeout=8);old_code=held.returncode;held=None
   if old_code==0 or 'result=unknown token_sent=1 send_kr=268435459' not in old_out or not alive(pid_b):raise AssertionError(f'old endpoint was not rejected: {old_out} {old_err}')
   fresh=must([client,service,str(pid_b),b['instance_id'],requirement,'wait-exit'],timeout=8)
   if 'result=accepted_exit_unverified' not in fresh.stdout or 'post_registration_identity_match=1' not in fresh.stdout or 'proc_exit_event=1 event_pid_matches=1 note_exit=1' not in fresh.stdout:raise AssertionError('replacement stop or exit observation failed')
   wait_for(lambda:not alive(pid_b) and not (config/'daemon.pid').exists())
   line=f'run={i} wrong_code_and_instance_no_token=1 apple_anchor_adhoc_refused_no_token=1 other_sender_rejected=1 normal_client_private_only=1 synthetic_public_stop_rejected=1 private_proof=1 first_stop_and_exit_observed=1 ssh_drain_tested={int(i==1)} replacement_distinct=1 old_private_invalid_dest=1 replacement_survived=1 fresh_stop_and_exit_observed=1\n';results.append(line);print(line,end='')
  finally:
   if held is not None:
    if held.poll() is None:held.kill()
    held.communicate(timeout=5)
   if ssh_request is not None:ssh_request.close()
   if sshd is not None:
    if sshd.poll() is None:sshd.terminate()
    try:sshd.wait(timeout=8)
    except subprocess.TimeoutExpired:sshd.kill();sshd.wait(timeout=5)
   if installed or state(service)!='absent':
    run(['launchctl','bootout',f'{DOMAIN}/{service}']);deadline=time.monotonic()+4
    while time.monotonic()<deadline and state(service)=='present':time.sleep(.05)
    if state(service)!='absent':keep=True;raise RuntimeError(f'test job not confirmed absent: {DOMAIN}/{service}; files retained: {root}, {config}')
   print('test_service_absent=1')
 # Crash A while launchd owns discovery, demonstrate that a restarted
 # instance preserves A's records, then use the explicit recovery command.
 config=Path(tempfile.mkdtemp(prefix='a2s-mach-lab-',dir='/tmp'));configs.append(config)
 (config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
 service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex[:12]}';plist=root/'crash.plist';err=root/'crash.err';port=free_port()
 variables={'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{port}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service,'A2S_MACH_LAB_LAUNCHD_DISCOVERY':'1'}
 plist.write_bytes(plistlib.dumps({'Label':service,'ProgramArguments':[str(binary)],'RunAtLoad':True,'MachServices':{service:True},'EnvironmentVariables':variables,'StandardOutPath':str(root/'crash.out'),'StandardErrorPath':str(err)}))
 installed=False
 try:
  must(['launchctl','bootstrap',DOMAIN,plist]);installed=True
  a=wait_for(lambda:identity(config,port));pid_a=a['pid'];instance=a['instance_id']
  paths=[config/'daemon.pid',config/'daemon.instance.json',config/'.daemon-ipc/s']
  snapshots=[paths[0].read_bytes(),paths[1].read_bytes()];socket_id=(paths[2].lstat().st_dev,paths[2].lstat().st_ino)
  cli_env=dict(env);cli_env['AGENT2SSH_CONFIG_DIR']=str(config);cli_env['AGENT2SSH_DAEMON_ADDR']=f'127.0.0.1:{port}'
  command=[str(args.cli.resolve(strict=True)),'daemon','clear-stale','--expect-pid',str(pid_a),'--expect-instance',instance]
  live=subprocess.run(command,capture_output=True,text=True,timeout=8,env=cli_env)
  if live.returncode==0 or not alive(pid_a):raise AssertionError('live daemon records were not protected')
  must(['launchctl','kill','SIGKILL',f'{DOMAIN}/{service}']);wait_for(lambda:not alive(pid_a))
  must([probe,'lookup',service,'send'])
  wait_for(lambda:err.exists() and 'already exists; retained for explicit recovery' in err.read_text(),seconds=25)
  if identity(config,port) is not None or paths[0].read_bytes()!=snapshots[0] or paths[1].read_bytes()!=snapshots[1] or (paths[2].lstat().st_dev,paths[2].lstat().st_ino)!=socket_id:
   raise AssertionError('crash records were overwritten or daemon unexpectedly active')
  must(['launchctl','bootout',f'{DOMAIN}/{service}']);installed=False
  wait_for(lambda:state(service)=='absent')
  wrong_command=[str(args.cli.resolve(strict=True)),'daemon','clear-stale','--expect-pid',str(pid_a),'--expect-instance','f'*32]
  wrong_clear=subprocess.run(wrong_command,capture_output=True,text=True,timeout=8,env=cli_env)
  if wrong_clear.returncode==0 or paths[0].read_bytes()!=snapshots[0] or paths[1].read_bytes()!=snapshots[1] or (paths[2].lstat().st_dev,paths[2].lstat().st_ino)!=socket_id:raise AssertionError('wrong instance removed or changed crash records')
  repaired=subprocess.run(command,capture_output=True,text=True,timeout=8,env=cli_env)
  if repaired.returncode or any(path.exists() for path in paths):raise AssertionError(f'explicit recovery failed: {repaired.stdout} {repaired.stderr}')
  must(['launchctl','bootstrap',DOMAIN,plist]);installed=True
  b=wait_for(lambda:identity(config,port));pid_b=b['pid']
  if pid_b==pid_a or b['instance_id']==instance:raise AssertionError('recovery did not start a distinct instance')
  stopped=must([client,service,str(pid_b),b['instance_id'],requirement,'stop'],timeout=8)
  if 'result=accepted_exit_unverified' not in stopped.stdout:raise AssertionError('recovered daemon did not accept private stop')
  wait_for(lambda:not alive(pid_b) and not (config/'daemon.pid').exists())
  line='crash_recovery live_clear_refused=1 crash_records_preserved=1 wrong_instance_clear_refused=1 explicit_clear=1 distinct_restart=1 private_stop=1\n';results.append(line);print(line,end='')
 finally:
  if installed or state(service)!='absent':
   run(['launchctl','bootout',f'{DOMAIN}/{service}']);deadline=time.monotonic()+4
   while time.monotonic()<deadline and state(service)=='present':time.sleep(.05)
   if state(service)!='absent':keep=True;raise RuntimeError(f'test job not confirmed absent: {DOMAIN}/{service}; files retained: {root}, {config}')
  print('test_service_absent=1')
 if args.archive:(ROOT/'launchd-daemon-results.txt').write_text(''.join(results))
finally:
 if not keep:
  shutil.rmtree(root)
  for config in configs:shutil.rmtree(config)
print('Verified isolated launchd real-daemon private endpoints; synthetic token only')
