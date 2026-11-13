#!/usr/bin/env python3
"""Exercise a feature-gated real daemon with only a synthetic Mach token.

The supplied lab binary is copied and ad-hoc signed in /tmp. No user daemon,
credentials, permanent launchd job or production Mach protocol is touched.
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

ROOT=Path(__file__).resolve().parent
REPO=ROOT.parents[3]
TOKEN='a2s-mach-lab-synthetic-token'
parser=argparse.ArgumentParser()
parser.add_argument('--daemon',type=Path,required=True)
parser.add_argument('--archive',action='store_true')
args=parser.parse_args()


def run(command,timeout=15):
 env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
 p=subprocess.run([str(x) for x in command],text=True,capture_output=True,timeout=timeout,env=env)
 if p.returncode: raise RuntimeError(f'{command}: exit={p.returncode}\n{p.stdout}\n{p.stderr}')
 return p


def free_port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]


def wait_health(proc,addr):
 opener=urllib.request.build_opener(urllib.request.ProxyHandler({}));deadline=time.monotonic()+15
 while time.monotonic()<deadline:
  if proc.poll() is not None:raise RuntimeError(f'lab daemon exited before ready: {proc.returncode}')
  try:
   with opener.open(f'http://127.0.0.1:{addr}/health',timeout=.5) as response:
    if json.load(response).get('pid')==proc.pid:return
  except (OSError,ValueError,urllib.error.URLError):pass
  time.sleep(.05)
 raise TimeoutError('lab daemon health timed out')


def invoke(client,service,pid,instance,requirement,mode,extra_env=None):
 env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
 if extra_env:env.update(extra_env)
 return subprocess.run([str(client),service,str(pid),instance,requirement,mode],capture_output=True,text=True,timeout=8,env=env)


def service_absent(lookup,service):
 deadline=time.monotonic()+5
 while True:
  r=subprocess.run([str(lookup),'lookup',service],capture_output=True,text=True,timeout=5)
  if r.returncode==4 and 'lookup_kr=1102' in r.stdout:return True
  if r.returncode!=0:raise RuntimeError(f'lookup error other than unknown service: {r.stdout} {r.stderr}')
  if time.monotonic()>=deadline:return False
  time.sleep(.05)


def start_local_exec(addr, config):
 marker=config/'lab-running-pids'
 command=f"sleep 8 & child=$!; printf '%s %s' $$ \"$child\" > '{marker}'; wait \"$child\""
 body=json.dumps({'host':'local','command':command,'force':True,'timeout_secs':20}).encode()
 request=socket.create_connection(('127.0.0.1',addr),timeout=3)
 request.sendall((f'POST /local-exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n').encode()+body)
 deadline=time.monotonic()+5
 while not marker.exists() and time.monotonic()<deadline:time.sleep(.02)
 if not marker.exists():request.close();raise RuntimeError('synthetic local command did not start')
 pids=[int(x) for x in marker.read_text().split()]
 if len(pids)!=2 or any(pid<=1 for pid in pids):request.close();raise RuntimeError('invalid test process markers')
 return request,pids


def processes_not_live(pids):
 for pid in pids:
  r=subprocess.run(['ps','-o','stat=','-p',str(pid)],capture_output=True,text=True,timeout=3)
  state=r.stdout.strip()
  if state and not state.startswith('Z'):return False
 return True


source=args.daemon.resolve(strict=True)
with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-tools-',dir='/tmp') as tools:
 root=Path(tools);binary=root/'daemon-mach-lab';client=root/'client';lookup=root/'lookup'
 shutil.copy2(source,binary)
 run(['codesign','--force','--sign','-','--options','runtime','--identifier','org.agent2ssh.research.machlab',binary])
 info=run(['codesign','-dv','--verbose=4',binary]).stderr
 match=re.search(r'^CDHash=([0-9a-f]{40})$',info,re.MULTILINE)
 if not match or 'Signature=adhoc' not in info or 'adhoc,runtime' not in info:raise RuntimeError('expected hardened ad-hoc lab binary')
 good=f'identifier "org.agent2ssh.research.machlab" and cdhash H"{match.group(1)}"'
 bad='identifier "org.agent2ssh.not-the-lab"'
 run(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',f'-I{REPO / "src-tauri/src/bin/agent2ssh_daemon/mach_lab"}',ROOT/'client.c','-framework','Security','-framework','CoreFoundation','-lbsm','-o',client])
 run(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',REPO/'docs/reports/evidence/macos-mach-discovery-2026-11-13/bootstrap_probe.c','-o',lookup])
 results=[]
 for i in range(1,4):
  with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
   config=Path(cd);(config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
   addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
   env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
   env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOCAL_SHELL':'/bin/sh','AGENT2SSH_LOG':'info','AGENT2SSH_LOG_FORMAT':'json','A2S_MACH_LAB_SERVICE':service})
   with tempfile.TemporaryFile(mode='w+t') as log:
    proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
    local_request=None
    try:
     wait_health(proc,addr);instance=json.loads((config/'daemon.instance.json').read_text())['instance_id']
     wrong=invoke(client,service,proc.pid,instance,bad,'stop')
     if wrong.returncode==0 or 'refused=code_or_task token_sent=0' not in wrong.stdout or proc.poll() is not None:raise AssertionError(f'wrong requirement did not fail closed: {wrong.stdout} {wrong.stderr}')
     mismatched=invoke(client,service,proc.pid,'f'*32,good,'stop')
     if mismatched.returncode==0 or 'token_sent=0' not in mismatched.stdout or proc.poll() is not None:raise AssertionError(f'wrong instance did not fail closed: {mismatched.stdout} {mismatched.stderr}')
     hello=invoke(client,service,proc.pid,instance,good,'hello-only')
     if hello.returncode or 'result=hello_only token_sent=0' not in hello.stdout or proc.poll() is not None:raise AssertionError(f'hello failed: {hello.stdout} {hello.stderr}')
     wrong_token=invoke(client,service,proc.pid,instance,good,'wrong-token')
     if wrong_token.returncode==0 or 'result=unknown' not in wrong_token.stdout or proc.poll() is not None:raise AssertionError(f'wrong token did not fail closed: {wrong_token.stdout} {wrong_token.stderr}')
     local_request,pids=start_local_exec(addr,config)
     stopped=invoke(client,service,proc.pid,instance,good,'wait-exit')
     if stopped.returncode or 'identity_observations_match=1 same_endpoint=1 runtime_policy_match=1' not in stopped.stdout or 'result=accepted_exit_unverified' not in stopped.stdout or 'post_registration_identity_match=1' not in stopped.stdout or 'proc_exit_event=1 event_pid_matches=1 note_exit=1' not in stopped.stdout:raise AssertionError(f'valid lab request failed: {stopped.stdout} {stopped.stderr}')
     if proc.wait(timeout=10)!=0:raise AssertionError('lab daemon exited with error')
     notification=re.search(r'task_name_exit_notification=([^\n]+)',stopped.stdout)
     if not notification or notification.group(1)!='dead_name':raise AssertionError(f'missing task name dead-name observation: {stopped.stdout}')
     log.seek(0);daemon_log=log.read()
     if '"trigger":"ipc"' not in daemon_log:raise AssertionError('daemon did not report existing IPC drain trigger')
     if any((config/name).exists() for name in ('daemon.pid','daemon.instance.json','.daemon-ipc/s')):raise AssertionError('daemon left its identity records')
     local_request.settimeout(5);response=bytearray()
     while True:
      chunk=local_request.recv(4096)
      if not chunk:break
      response.extend(chunk)
     local_request.close();local_request=None
     audit=(config/'audit.jsonl').read_text()
     if 'local command stopped during daemon shutdown' not in audit:raise AssertionError('missing local command shutdown audit')
     deadline=time.monotonic()+5
     while not processes_not_live(pids) and time.monotonic()<deadline:time.sleep(.05)
     if not processes_not_live(pids):raise AssertionError('test local process tree still live')
     line=f'run={i} wrong_code_no_token=1 wrong_instance_no_token=1 hello_no_token=1 wrong_token_no_stop=1 verified_same_endpoint=1 accepted_exit_unverified=1 child_exited=1 task_name_notification={notification.group(1).replace(" ",",")} proc_registration_rechecked=1 proc_exit_event=1 drain_ipc=1 local_exec_cancelled=1 local_tree_not_live=1 records_removed=1\n';results.append(line);print(line,end='')
    finally:
     if local_request is not None:local_request.close()
     if proc.poll() is None:proc.terminate()
     try:proc.wait(timeout=8)
     except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
     if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
     print('test_service_absent=1')
 # A real SSH request uses an unprivileged loopback sshd, synthetic keys and
 # an isolated remote HOME. Neither user SSH configuration nor credentials
 # are used. An interrupted remote outcome must remain explicitly unknown.
 with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
  config=Path(cd); key=config/'key'; hostkey=config/'hostkey'; authorized=config/'authorized_keys'
  for path in (key,hostkey):run(['ssh-keygen','-q','-t','ed25519','-N','','-f',path])
  authorized.write_bytes(key.with_suffix('.pub').read_bytes());authorized.chmod(0o600)
  sshport=free_port();sshd_config=config/'sshd_config'
  sshd_config.write_text(f'Port {sshport}\nListenAddress 127.0.0.1\nHostKey {hostkey}\nPidFile {config}/sshd.pid\nAuthorizedKeysFile {authorized}\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nPermitUserRC no\nSetEnv HOME={config} ZDOTDIR={config}\n')
  user=pwd.getpwuid(os.getuid()).pw_name
  (config/'hosts.json').write_text(json.dumps({'schema_version':1,'hosts':[{'name':'fixture','host':'127.0.0.1','port':sshport,'user':user,'key_path':str(key)}]}))
  (config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
  addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
  env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
  env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'info','AGENT2SSH_LOG_FORMAT':'json','A2S_MACH_LAB_SERVICE':service})
  with tempfile.TemporaryFile(mode='w+t') as log:
   sshd=subprocess.Popen(['/usr/sbin/sshd','-D','-e','-f',str(sshd_config)],stdin=subprocess.DEVNULL,stdout=log,stderr=log)
   proc=None;request=None
   try:
    time.sleep(.4)
    probe=run(['ssh','-F','/dev/null','-o','StrictHostKeyChecking=no','-o','UserKnownHostsFile=/dev/null','-o','IdentitiesOnly=yes','-o','BatchMode=yes','-i',key,'-p',str(sshport),f'{user}@127.0.0.1','printf "%s\\n%s\\n" "$HOME" "$ZDOTDIR"'])
    if probe.stdout.splitlines()!=[str(config),str(config)]:raise AssertionError('remote shell HOME was not isolated')
    proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log);wait_health(proc,addr)
    instance=json.loads((config/'daemon.instance.json').read_text())['instance_id'];marker=config/'remote-ready'
    command=f"printf ready > '{marker}'; sleep 8"
    body=json.dumps({'host':'fixture','command':command,'force':True,'timeout_secs':20,'reason':'test change','side_effect':'test effect','change_id':'test-42'}).encode()
    request=socket.create_connection(('127.0.0.1',addr),timeout=3)
    request.sendall((f'POST /exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n').encode()+body)
    deadline=time.monotonic()+12
    while not marker.exists() and proc.poll() is None and time.monotonic()<deadline:time.sleep(.02)
    if not marker.exists():
     request.settimeout(1);raise AssertionError(f'SSH command did not start: {request.recv(4096)!r}')
    stopped=invoke(client,service,proc.pid,instance,good,'stop')
    if stopped.returncode or 'result=accepted_exit_unverified' not in stopped.stdout:raise AssertionError(f'Mach stop failed: {stopped.stdout} {stopped.stderr}')
    if proc.wait(timeout=10)!=0:raise AssertionError('daemon failed after SSH drain')
    request.settimeout(5);response=bytearray()
    while True:
     chunk=request.recv(4096)
     if not chunk:break
     response.extend(chunk)
    if not response.startswith(b'HTTP/1.1 503') or b'remote command outcome unknown' not in response:raise AssertionError(f'unexpected SSH drain response: {response!r}')
    audit=[json.loads(line) for line in (config/'audit.jsonl').read_text().splitlines()]
    entry=next((entry for entry in audit if entry.get('change_id')=='test-42'),None)
    if not entry or 'remote command outcome unknown' not in entry.get('reason','') or 'test change' not in entry['reason'] or entry.get('side_effect')!='test effect':raise AssertionError('SSH interrupted audit mismatch')
    line='ssh_drain real_daemon=1 ssh_started=1 response_503_outcome_unknown=1 audit_unknown=1 exit_observed=1\n';results.append(line);print(line,end='')
   finally:
    if request is not None:request.close()
    for child in (proc,sshd):
     if child is not None:
      if child.poll() is None:child.terminate()
      try:child.wait(timeout=8)
      except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
    if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
    print('test_service_absent=1')
 # A known CDHash is not sufficient when runtime hardening is absent or
 # exceptions allow injected/modified code. All these are synthetic builds.
 policies=[('no-runtime',False,None),
           ('allow-dyld',True,'com.apple.security.cs.allow-dyld-environment-variables'),
           ('disable-validation',True,'com.apple.security.cs.disable-library-validation'),
           ('unsigned-memory',True,'com.apple.security.cs.allow-unsigned-executable-memory'),
           ('get-task-allow',True,'com.apple.security.get-task-allow')]
 for label,runtime,entitlement in policies:
  candidate=root/f'daemon-{label}';shutil.copy2(source,candidate)
  sign=['codesign','--force','--sign','-','--identifier','org.agent2ssh.research.machlab']
  if runtime:sign+=['--options','runtime']
  if entitlement:
   plist=root/f'{label}.plist';plist.write_bytes(plistlib.dumps({entitlement:True}));sign+=['--entitlements',plist]
  run(sign+[candidate]);candidate_info=run(['codesign','-dv','--verbose=4',candidate]).stderr
  candidate_hash=re.search(r'^CDHash=([0-9a-f]{40})$',candidate_info,re.MULTILINE)
  if not candidate_hash:raise RuntimeError(f'no candidate CDHash for {label}')
  requirement=f'identifier "org.agent2ssh.research.machlab" and cdhash H"{candidate_hash.group(1)}"'
  with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
   config=Path(cd);(config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
   addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
   env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
   env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service})
   with tempfile.TemporaryFile(mode='w+t') as log:
    proc=subprocess.Popen([str(candidate)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
    try:
     wait_health(proc,addr);instance=json.loads((config/'daemon.instance.json').read_text())['instance_id']
     refusal=invoke(client,service,proc.pid,instance,requirement,'stop')
     if refusal.returncode==0 or 'refused=code_or_task token_sent=0' not in refusal.stdout or proc.poll() is not None:
      raise AssertionError(f'unsafe runtime policy was not refused: {label}: {refusal.stdout} {refusal.stderr}')
     line=f'policy={label} no_token=1 daemon_alive=1\n';results.append(line);print(line,end='')
    finally:
     if proc.poll() is None:proc.terminate()
     try:proc.wait(timeout=8)
     except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
     if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
     print('test_service_absent=1')
 # Destroying the reply port immediately after sending races with server
 # validation: either outcome is possible. The client must report Unknown.
 with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
  config=Path(cd);(config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
  addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
  env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
  env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service})
  with tempfile.TemporaryFile(mode='w+t') as log:
   proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
   try:
    wait_health(proc,addr);instance=json.loads((config/'daemon.instance.json').read_text())['instance_id']
    lost=invoke(client,service,proc.pid,instance,good,'drop-reply')
    if lost.returncode==0 or 'result=unknown reply_destroyed=1' not in lost.stdout:raise AssertionError(f'lost reply was not unknown: {lost.stdout} {lost.stderr}')
    try:
     if proc.wait(timeout=3)!=0:raise AssertionError('daemon exited with error after reply destruction')
     outcome='exited'
    except subprocess.TimeoutExpired:outcome='still_running'
    line=f'lost_reply client_unknown=1 observed_daemon={outcome}\n';results.append(line);print(line,end='')
   finally:
    if proc.poll() is None:proc.terminate()
    try:proc.wait(timeout=8)
    except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
    if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
    print('test_service_absent=1')
 # Deterministically destroy the reply port after the server has consumed the
 # authorized request but before it sends. No retry is sent by the client.
 with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
  config=Path(cd);(config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
  addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}';barrier=config/'reply-barrier'
  env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
  env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service,'A2S_MACH_LAB_REPLY_BARRIER':str(barrier)})
  with tempfile.TemporaryFile(mode='w+t') as log:
   proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log);held=None
   try:
    wait_health(proc,addr);instance=json.loads((config/'daemon.instance.json').read_text())['instance_id']
    held=subprocess.Popen([str(client),service,str(proc.pid),instance,good,'hold-reply'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    accepted=Path(str(barrier)+'.accepted');deadline=time.monotonic()+4
    while not accepted.exists() and held.poll() is None and proc.poll() is None and time.monotonic()<deadline:time.sleep(.01)
    if not accepted.exists():raise AssertionError('server did not reach post-authorization reply barrier')
    out,err=held.communicate(input='x',timeout=4)
    if held.returncode==0 or 'result=unknown reply_destroyed=1' not in out:raise AssertionError(f'client did not report unknown: {out} {err}')
    held=None;Path(str(barrier)+'.release').write_text('release')
    if proc.wait(timeout=8)!=0:raise AssertionError('daemon did not exit after accepted reply was lost')
    line='accepted_reply_lost client_unknown=1 daemon_exit_observed=1 no_retry=1\n';results.append(line);print(line,end='')
   finally:
    if held is not None:
     if held.poll() is None:held.kill()
     held.communicate(timeout=5)
    if proc.poll() is None:proc.terminate()
    try:proc.wait(timeout=8)
    except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
    if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
    print('test_service_absent=1')
 # The bounded hello table must fail closed when full, then allow a fresh
 # client after expiry. Every request in this test is secret-free.
 with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
  config=Path(cd);(config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
  addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
  env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
  env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service})
  with tempfile.TemporaryFile(mode='w+t') as log:
   proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
   try:
    wait_health(proc,addr);instance=json.loads((config/'daemon.instance.json').read_text())['instance_id']
    for _ in range(32):
     hello=invoke(client,service,proc.pid,instance,good,'hello-only')
     if hello.returncode or 'result=hello_only token_sent=0' not in hello.stdout:raise AssertionError('hello table filled early')
    full=invoke(client,service,proc.pid,instance,good,'hello-only')
    if full.returncode==0 or 'token_sent=0' not in full.stdout or proc.poll() is not None:raise AssertionError('full hello table did not fail closed')
    time.sleep(10.1)
    fresh=invoke(client,service,proc.pid,instance,good,'stop')
    if fresh.returncode or 'result=accepted_exit_unverified' not in fresh.stdout:raise AssertionError('expired sessions did not release capacity')
    if proc.wait(timeout=8)!=0:raise AssertionError('daemon failed after capacity recovery')
    line='hello_capacity=32 full_refused_no_token=1 expired_capacity_recovered=1\n';results.append(line);print(line,end='')
   finally:
    if proc.poll() is None:proc.terminate()
    try:proc.wait(timeout=8)
    except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
    if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
    print('test_service_absent=1')
 # The opt-in binary must not register the lab endpoint with an ordinary
 # credential or without its dedicated temporary-directory marker.
 for label,marker_value,token_value in [('missing-marker',False,TOKEN),('wrong-token',True,'different-synthetic-token')]:
  with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cd:
   config=Path(cd)
   if marker_value:(config/'mach-lab-only').write_bytes(b'synthetic-only')
   (config/'daemon.token').write_text(token_value);(config/'daemon.token').chmod(0o600)
   addr=free_port();service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
   env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
   env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'error','A2S_MACH_LAB_SERVICE':service})
   proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
   try:
    out,err=proc.communicate(timeout=8)
    if proc.returncode==0:raise AssertionError(f'lab safety gate accepted {label}')
    if label=='wrong-token' and 'fixed synthetic token' not in err:raise AssertionError(f'unexpected refusal: {err}')
    if not service_absent(lookup,service):raise AssertionError('lab service registered despite failed safety gate')
    line=f'lab_gate={label} refused=1 service_absent=1\n';results.append(line);print(line,end='')
   finally:
    if proc.poll() is None:proc.terminate()
    try:proc.wait(timeout=8)
    except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
    if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
 # A client retains A's verified send right while A dies and B registers the
 # same name. Its one pending request must not be delivered to B.
 for i in range(1,4):
  with tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as ca, tempfile.TemporaryDirectory(prefix='a2s-mach-lab-',dir='/tmp') as cb:
   config_a,config_b=Path(ca),Path(cb)
   for config in (config_a,config_b):
    (config/'mach-lab-only').write_bytes(b'synthetic-only');(config/'daemon.token').write_text(TOKEN);(config/'daemon.token').chmod(0o600)
   service=f'org.agent2ssh.lab.{os.getpid()}.{uuid.uuid4().hex}'
   clean={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_','A2S_MACH_LAB_'))}
   def start(config):
    addr=free_port();env=dict(clean);env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{addr}','AGENT2SSH_LOG':'info','AGENT2SSH_LOG_FORMAT':'json','A2S_MACH_LAB_SERVICE':service})
    log=(config/'lab-output.log').open('w+');proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
    try:wait_health(proc,addr)
    except Exception:
     if proc.poll() is None:proc.terminate()
     proc.wait(timeout=8);log.close();raise
    return proc,log
   a=b=held=registration_held=None;log_a=log_b=None
   try:
    a,log_a=start(config_a);record_a=json.loads((config_a/'daemon.instance.json').read_text());pid_a=a.pid
    registration_ready=root/f'registration-held-{i}';registration_env=dict(clean);registration_env['A2S_MACH_LAB_CLIENT_READY']=str(registration_ready)
    registration_held=subprocess.Popen([str(client),service,str(pid_a),record_a['instance_id'],good,'hold-registration'],env=registration_env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    deadline=time.monotonic()+8
    while not registration_ready.exists() and registration_held.poll() is None and time.monotonic()<deadline:time.sleep(.02)
    if not registration_ready.exists():raise RuntimeError('client did not reach post-registration barrier')
    ready=root/f'held-client-{i}';env=dict(clean);env['A2S_MACH_LAB_CLIENT_READY']=str(ready)
    held=subprocess.Popen([str(client),service,str(pid_a),record_a['instance_id'],good,'hold'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    deadline=time.monotonic()+8
    while not ready.exists() and held.poll() is None and time.monotonic()<deadline:time.sleep(.02)
    if not ready.exists():raise RuntimeError('held client did not complete identity checks')
    nonce=ready.read_text().strip()
    if not re.fullmatch(r'[0-9a-f]{32}',nonce):raise AssertionError('invalid held-client nonce')
    attacker=invoke(client,service,pid_a,record_a['instance_id'],good,'adversarial-stop',{'A2S_MACH_LAB_TEST_NONCE':nonce})
    if attacker.returncode==0 or 'adversarial_stop=refused' not in attacker.stdout or a.poll() is not None:
     raise AssertionError(f'different sender was not rejected: {attacker.stdout} {attacker.stderr}')
    reexec=invoke(client,service,pid_a,record_a['instance_id'],good,'exec-attack')
    before=re.search(r'sender_before_exec_pid=(\d+)',reexec.stdout);after=re.search(r'sender_after_exec_pid=(\d+)',reexec.stdout)
    if reexec.returncode==0 or 'adversarial_stop=refused' not in reexec.stdout or not before or not after or before.group(1)!=after.group(1) or a.poll() is not None:
     raise AssertionError(f're-exec sender was not rejected: {reexec.stdout} {reexec.stderr}')
    if 'A2S_MACH_LAB_STOP_RECEIVED' not in (config_a/'lab-output.log').read_text():raise AssertionError('adversarial test did not reach server')
    a.kill();a.wait(timeout=8)
    if not service_absent(lookup,service):raise RuntimeError('old service name did not disappear')
    b,log_b=start(config_b);record_b=json.loads((config_b/'daemon.instance.json').read_text());pid_b=b.pid
    if pid_a==pid_b or record_a['instance_id']==record_b['instance_id']:raise AssertionError('replacement was not a distinct instance')
    reg_out,reg_err=registration_held.communicate(input='x',timeout=8)
    if registration_held.returncode==0 or 'refused=post_registration_identity token_sent=0' not in reg_out:
     raise AssertionError(f'changed target was not refused before token: {reg_out} {reg_err}')
    registration_held=None
    old_out,old_err=held.communicate(input='x',timeout=8)
    if held.returncode==0 or 'result=unknown token_sent=1' not in old_out or 'result=accepted_exit_unverified' in old_out:
     raise AssertionError(f'old endpoint was not conservatively rejected: {old_out} {old_err}')
    held=None
    if b.poll() is not None:raise AssertionError('replacement daemon exited after old request')
    time.sleep(.1)
    if 'A2S_MACH_LAB_STOP_RECEIVED' in (config_b/'lab-output.log').read_text():raise AssertionError('replacement received the old stop/token')
    if not (config_a/'daemon.pid').exists() or not (config_b/'daemon.pid').exists():raise AssertionError('unexpected PID cleanup after A crash')
    fresh=invoke(client,service,pid_b,record_b['instance_id'],good,'stop')
    if fresh.returncode or 'result=accepted_exit_unverified' not in fresh.stdout:raise AssertionError(f'fresh replacement stop failed: {fresh.stdout} {fresh.stderr}')
    if b.wait(timeout=10)!=0:raise AssertionError('replacement daemon exited with error')
    line=f'handoff_run={i} different_sender_rejected=1 same_pid_reexec_rejected=1 registration_race_no_token=1 old_right_rejected=1 replacement_received_no_old_stop=1 replacement_survived=1 stale_a_preserved=1 fresh_stop_accepted=1\n';results.append(line);print(line,end='')
   finally:
    if held is not None:
     if held.poll() is None:held.kill()
     held.communicate(timeout=5)
    if registration_held is not None:
     if registration_held.poll() is None:registration_held.kill()
     registration_held.communicate(timeout=5)
    for proc in (a,b):
     if proc is not None:
      if proc.poll() is None:proc.terminate()
      try:proc.wait(timeout=8)
      except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
    for log in (log_a,log_b):
     if log is not None:log.close()
    if not service_absent(lookup,service):raise RuntimeError(f'test Mach service remains: {service}')
    print('test_service_absent=1')
 if args.archive:(ROOT/'results.txt').write_text(''.join(results))
print('Verified 3 successful cycles, 5 policy refusals, 2 lab safety gates and 3 service replacements; ad-hoc signing, synthetic token only')
