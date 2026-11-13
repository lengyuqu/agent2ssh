#!/usr/bin/env python3
"""Test launchd discovery plus a guarded per-instance endpoint in /tmp."""
from pathlib import Path
import argparse
import os
import plistlib
import re
import shutil
import subprocess
import tempfile
import time
import uuid

ROOT=Path(__file__).resolve().parent;DOMAIN=f'gui/{os.getuid()}'
parser=argparse.ArgumentParser();parser.add_argument('--archive',action='store_true');args=parser.parse_args()
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','AGENT2SSH_'))}
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
def wait_for(predicate,seconds=8):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  if predicate():return
  time.sleep(.03)
 raise TimeoutError('test condition not observed')
def alive(pid):
 p=run(['ps','-o','stat=','-p',str(pid)])
 return bool(p.stdout.strip()) and not p.stdout.strip().startswith('Z')
root=Path(tempfile.mkdtemp(prefix='a2s-launchd-private-',dir='/tmp'));keep=False
try:
 binary=root/'private';probe=root/'lookup'
 must(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',ROOT/'launchd_private.c','-framework','Security','-framework','CoreFoundation','-lbsm','-o',binary])
 must(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',ROOT/'bootstrap_probe.c','-o',probe])
 must(['codesign','--force','--sign','-','--options','runtime','--identifier','org.agent2ssh.research.private',binary])
 signature=must(['codesign','-dv','--verbose=4',binary]).stderr;cdhash=re.search(r'^CDHash=([0-9a-f]{40})$',signature,re.MULTILINE)
 if not cdhash or 'Signature=adhoc' not in signature:raise AssertionError('expected an ad-hoc signed test image')
 requirement=f'identifier "org.agent2ssh.research.private" and cdhash H"{cdhash.group(1)}"'
 results=[]
 for i in range(1,4):
  label=f'org.agent2ssh.test.private.{os.getpid()}.{uuid.uuid4().hex[:12]}'
  plist=root/f'{i}.plist';log=root/f'{i}.log';ready=root/f'{i}.ready';release=root/f'release{i}';release.mkdir()
  plist.write_bytes(plistlib.dumps({'Label':label,'ProgramArguments':[str(binary),'server',label,str(log),str(release)],'RunAtLoad':True,'MachServices':{label:True},'StandardOutPath':str(root/f'{i}.out'),'StandardErrorPath':str(root/f'{i}.err')}))
  held=None;installed=False
  try:
   must(['launchctl','bootstrap',DOMAIN,plist]);installed=True
   held=subprocess.Popen([str(binary),'client',label,str(ready),requirement],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env)
   wait_for(lambda:ready.exists() or held.poll() is not None,seconds=12)
   if not ready.exists():raise AssertionError(f'private client failed: {held.communicate()}')
   first=int(ready.read_text());wait_for(lambda:log.exists() and f'private_issued pid={first}' in log.read_text())
   (release/str(first)).write_text('release')
   wait_for(lambda:not alive(first))
   must([probe,'lookup',label,'send'])
   wait_for(lambda:bool(re.search(r'replacement_started pid=\d+',log.read_text())),seconds=20)
   match=re.search(r'replacement_started pid=(\d+)',log.read_text());second=int(match.group(1))
   if first==second or not alive(second):raise AssertionError('replacement was not live and distinct')
   stdout,stderr=held.communicate(input='x',timeout=8);code=held.returncode;held=None
   if code or 'private_old_send_kr=268435459' not in stdout or not alive(second):raise AssertionError(f'old private endpoint unsafe: {stdout} {stderr}')
   line=f'run={i} task_audit_matched=1 pinned_code_matched=1 old_instance_exited=1 replacement_distinct_live=1 old_private_send_invalid_dest=1\n';results.append(line);print(line,end='')
  finally:
   if held is not None:
    if held.poll() is None:held.kill()
    held.communicate(timeout=5)
   if installed or state(label)!='absent':
    run(['launchctl','bootout',f'{DOMAIN}/{label}']);deadline=time.monotonic()+4
    while time.monotonic()<deadline and state(label)=='present':time.sleep(.05)
    if state(label)!='absent':keep=True;raise RuntimeError(f'test service not confirmed absent: {DOMAIN}/{label}; files retained: {root}')
   print('test_service_absent=1')
 if args.archive:(ROOT/'launchd-private-results.txt').write_text(''.join(results))
finally:
 if not keep:shutil.rmtree(root)
print('Verified temporary launchd private-endpoint experiment; synthetic messages only')
