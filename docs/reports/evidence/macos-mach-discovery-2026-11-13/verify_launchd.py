#!/usr/bin/env python3
"""Probe same send-right behavior for transient per-user launchd jobs.

All labels, binaries, payloads and files are synthetic. Every job is booted out;
on uncertain cleanup, preserve its files and report the exact service label.
"""
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

ROOT=Path(__file__).resolve().parent
DOMAIN=f'gui/{os.getuid()}'
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

root=Path(tempfile.mkdtemp(prefix='a2s-launchd-mach-',dir='/tmp'));keep=False
try:
 server=root/'server';client=root/'client'
 must(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',ROOT/'launchd_service.c','-o',server])
 must(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',ROOT/'bootstrap_probe.c','-o',client])
 must(['codesign','--force','--sign','-','--identifier','org.agent2ssh.research.launchdmach',server])
 results=[]
 for i in range(1,4):
  label=f'org.agent2ssh.test.mach.{os.getpid()}.{uuid.uuid4().hex[:12]}'
  service=label;plist=root/f'{i}.plist';log=root/f'{i}.log';ready=root/f'{i}.ready'
  plist.write_bytes(plistlib.dumps({'Label':label,'ProgramArguments':[str(server),service,str(log)],'RunAtLoad':True,'MachServices':{service:True},'StandardOutPath':str(root/f'{i}.out'),'StandardErrorPath':str(root/f'{i}.err')}))
  held=None;installed=False
  try:
   must(['launchctl','bootstrap',DOMAIN,plist]);installed=True
   wait_for(lambda:log.exists() and 'start pid=' in log.read_text())
   started=log.read_text();match=re.search(r'start pid=(\d+) checkin=(\d+) guard=(\d+)',started)
   if not match or match.group(2)!='0' or match.group(3)!='0':raise AssertionError(f'launchd checkin or guard failed: {started}')
   held=subprocess.Popen([str(client),'lookup',service,'hold',str(ready)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env)
   wait_for(lambda:ready.exists() or held.poll() is not None)
   if not ready.exists():raise AssertionError('old sender lookup failed')
   must([client,'lookup',service,'send'])
   wait_for(lambda:bool(re.search(r'message pid=\d+ kr=0 id=4811',log.read_text())))
   # Wait for the first owned launchd service to finish without signalling it.
   first_pid=int(match.group(1))
   def first_exited():
    p=run(['ps','-o','stat=','-p',str(first_pid)])
    return not p.stdout.strip() or p.stdout.strip().startswith('Z')
   wait_for(first_exited)
   old_out,old_err=held.communicate(input='x',timeout=8);held=None
   old=re.search(r'old_send_kr=(\d+) new_lookup_kr=(\d+) new_send_kr=(\d+)',old_out)
   if not old:raise AssertionError(f'old sender result missing: {old_out} {old_err}')
   print(f'old_send_kr={old.group(1)} new_lookup_kr={old.group(2)} new_send_kr={old.group(3)}',flush=True)
   # The lookup helper intentionally exits nonzero when the old right remains
   # live, so record its values rather than assuming either outcome.
   if old.group(1)=='0':
    try:wait_for(lambda:bool(re.search(r'message pid=\d+ kr=0 id=4812',log.read_text())),seconds=20)
    except TimeoutError:pass
   observations=log.read_text();message=re.search(r'message pid=(\d+) kr=0 id=4812',observations)
   print(f'service_observations={observations.strip()!r}',flush=True)
   if message and int(message.group(1))==first_pid:raise AssertionError('old message unexpectedly reached first exited process')
   line=f'run={i} first_exited=1 old_send_kr={old.group(1)} new_lookup_kr={old.group(2)} new_send_kr={old.group(3)} replacement_received_old={int(message is not None)}\n';results.append(line);print(line,end='')
  finally:
   if held is not None:
    if held.poll() is None:held.kill()
    held.communicate(timeout=5)
   if installed or state(label)!='absent':
    run(['launchctl','bootout',f'{DOMAIN}/{label}'])
    deadline=time.monotonic()+4
    while time.monotonic()<deadline and state(label)=='present':time.sleep(.05)
    if state(label)!='absent':keep=True;raise RuntimeError(f'test service not confirmed absent: {DOMAIN}/{label}; files retained: {root}')
   print('test_service_absent=1')
 if args.archive:(ROOT/'launchd-results.txt').write_text(''.join(results))
finally:
 if not keep:shutil.rmtree(root)
print('Verified transient launchd job cleanup; ad-hoc signatures and synthetic messages only')
