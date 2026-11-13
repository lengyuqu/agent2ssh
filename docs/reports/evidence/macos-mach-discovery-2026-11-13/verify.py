#!/usr/bin/env python3
"""Isolated bootstrap registration/recovery experiments; no real daemon.

The SIGKILL case terminates only a test process launched by this script.
Names are random, unprivileged and transient; no launchd plist is installed.
"""
from pathlib import Path
import argparse
import os
import subprocess
import tempfile
import time
import uuid

ROOT=Path(__file__).resolve().parent
parser=argparse.ArgumentParser()
parser.add_argument('--archive',action='store_true')
args=parser.parse_args()


def command(args,timeout=8):
 return subprocess.run(args,text=True,capture_output=True,timeout=timeout)


def start(binary,name,ready):
 p=subprocess.Popen([str(binary),'server',name,str(ready)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
 limit=time.monotonic()+5
 while not ready.exists() and p.poll() is None and time.monotonic()<limit:
  time.sleep(.01)
 if not ready.exists():
  if p.poll() is None:p.kill()
  out,err=p.communicate(timeout=5)
  raise RuntimeError(f'server did not register: {out} {err}')
 return p,int(ready.read_text().strip())


def cleanup(p):
 if p is None:return
 if p.poll() is None:p.kill()
 p.communicate(timeout=5)


with tempfile.TemporaryDirectory(prefix='a2s-mach-discovery-',dir='/tmp') as t:
 root=Path(t);binary=root/'probe'
 r=command(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-Wno-deprecated-declarations','-O1',str(ROOT/'bootstrap_probe.c'),'-o',str(binary)])
 if r.returncode: raise RuntimeError(r.stderr)
 results=[]
 for mode in ('normal','sigkill'):
  for index in range(1,4):
   name=f'org.agent2ssh.test.discovery.{os.getpid()}.{uuid.uuid4().hex}'
   a=b=duplicate=holder=None
   try:
    a,kr=start(binary,name,root/f'{mode}-{index}-a')
    if kr:raise RuntimeError(f'first registration failed {kr}')
    client=command([str(binary),'lookup',name,'send'])
    if client.returncode or 'lookup_kr=0' not in client.stdout or 'send_kr=0' not in client.stdout:
     raise RuntimeError(f'first lookup/send failed {client.stdout} {client.stderr}')
    holder_ready=root/f'{mode}-{index}-holder'
    holder=subprocess.Popen([str(binary),'lookup',name,'hold',str(holder_ready)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    deadline=time.monotonic()+5
    while not holder_ready.exists() and holder.poll() is None and time.monotonic()<deadline: time.sleep(.01)
    if not holder_ready.exists(): raise RuntimeError('client did not obtain the old send right')
    duplicate,dup_kr=start(binary,name,root/f'{mode}-{index}-duplicate')
    if dup_kr==0:raise RuntimeError('second live registration unexpectedly succeeded')
    dup_out,dup_err=duplicate.communicate(timeout=5); duplicate=None
    if mode=='normal':
     a_out,a_err=a.communicate(input='x',timeout=5)
     if a.returncode:raise RuntimeError(f'normal server failed {a_out} {a_err}')
    else:
     a.kill();a_out,a_err=a.communicate(timeout=5)
     if a.returncode!=-9:raise RuntimeError(f'expected killed child, got {a.returncode}')
    a=None
    deadline=time.monotonic()+5; attempts=0
    while True:
     probe=command([str(binary),'lookup',name]);attempts+=1
     if probe.returncode!=0:
      if probe.returncode!=4 or 'lookup_kr=1102' not in probe.stdout:
       raise RuntimeError(f'lookup failed for reason other than unknown service: {probe.stdout} {probe.stderr}')
      break
     if time.monotonic()>=deadline:raise RuntimeError(f'service name remains after exit: {name}')
     time.sleep(.05)
    b,second_kr=start(binary,name,root/f'{mode}-{index}-b')
    if second_kr:raise RuntimeError(f'registration after exit failed: {second_kr}')
    holder_out,holder_err=holder.communicate(input='x',timeout=5)
    if holder.returncode or 'new_lookup_kr=0 new_send_kr=0' not in holder_out or 'old_send_kr=0' in holder_out:
     raise RuntimeError(f'old right redirected or new lookup failed: {holder_out} {holder_err}')
    holder=None
    b_out,b_err=b.communicate(input='x',timeout=5)
    if b.returncode:raise RuntimeError(f'second server failed: {b_out} {b_err}')
    b=None
    deadline=time.monotonic()+5
    while True:
     final=command([str(binary),'lookup',name])
     if final.returncode!=0:
      if final.returncode!=4 or 'lookup_kr=1102' not in final.stdout:
       raise RuntimeError(f'final lookup failed for a reason other than unknown service: {final.stdout} {final.stderr}')
      break
     if time.monotonic()>=deadline:raise RuntimeError(f'service remains after final exit: {name}')
     time.sleep(.05)
    s=f'{mode} run={index} first_register=0 first_lookup=0 duplicate_register={dup_kr} after_exit_lookup_rc={probe.returncode} polls={attempts} reregister=0 held_right_rejected=1 second_lookup=0 final_absent=1\n{holder_out}'
    print(s,end='');results.append(s)
   finally:
    for p in (a,b,duplicate,holder):cleanup(p)
 if args.archive:(ROOT/'results.txt').write_text(''.join(results))
print('Verified 3 normal and 3 SIGKILL recovery cases')
