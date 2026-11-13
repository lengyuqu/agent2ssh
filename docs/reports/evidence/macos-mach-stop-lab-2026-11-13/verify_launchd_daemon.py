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
parser=argparse.ArgumentParser();parser.add_argument('--daemon',type=Path,required=True);parser.add_argument('--archive',action='store_true');args=parser.parse_args()
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
  installed=False;held=None
  try:
   must(['launchctl','bootstrap',DOMAIN,plist]);installed=True
   a=wait_for(lambda:identity(config,port));pid_a=a['pid']
   wrong=run([client,service,str(pid_a),a['instance_id'],'identifier "org.agent2ssh.wrong"','stop'])
   if wrong.returncode==0 or 'refused=code_or_task token_sent=0' not in wrong.stdout or not alive(pid_a):raise AssertionError(f'wrong code did not fail closed: {wrong.stdout} {wrong.stderr}')
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
   stopped=must([client,service,str(pid_a),a['instance_id'],requirement,'stop'],timeout=8)
   if 'result=accepted_exit_unverified' not in stopped.stdout:raise AssertionError('first stop not accepted')
   wait_for(lambda:not alive(pid_a) and not (config/'daemon.pid').exists())
   must([probe,'lookup',service,'send']) # synthetic public discovery wake-up
   b=wait_for(lambda:(v if (v:=identity(config,port)) and v['pid']!=pid_a else None),seconds=25);pid_b=b['pid']
   old_out,old_err=held.communicate(input='x',timeout=8);old_code=held.returncode;held=None
   if old_code==0 or 'result=unknown token_sent=1 send_kr=268435459' not in old_out or not alive(pid_b):raise AssertionError(f'old endpoint was not rejected: {old_out} {old_err}')
   fresh=must([client,service,str(pid_b),b['instance_id'],requirement,'stop'],timeout=8)
   if 'result=accepted_exit_unverified' not in fresh.stdout:raise AssertionError('replacement stop not accepted')
   wait_for(lambda:not alive(pid_b) and not (config/'daemon.pid').exists())
   line=f'run={i} wrong_code_and_instance_no_token=1 other_sender_rejected=1 normal_client_private_only=1 synthetic_public_stop_rejected=1 private_proof=1 first_stop_accepted=1 replacement_distinct=1 old_private_invalid_dest=1 replacement_survived=1 fresh_stop_accepted=1\n';results.append(line);print(line,end='')
  finally:
   if held is not None:
    if held.poll() is None:held.kill()
    held.communicate(timeout=5)
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
