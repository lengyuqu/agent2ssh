#!/usr/bin/env python3
"""Test CDHash and dyld behavior on isolated copies of a real daemon binary.

The source binary is copied, never modified. All daemons use temporary configs,
loopback ports and a synthetic token. Only processes spawned here are signalled.
"""
from pathlib import Path
import argparse
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
import json

ROOT=Path(__file__).resolve().parent
MARKER='A2S_SYNTHETIC_INJECTED_LIBRARY'
parser=argparse.ArgumentParser()
parser.add_argument('--daemon',type=Path,required=True,help='path to a built agent2ssh-daemon; it is copied before signing/running')
parser.add_argument('--archive',action='store_true')
args=parser.parse_args()


def run(command,timeout=30):
 env={key:value for key,value in os.environ.items() if not key.startswith(('DYLD_','AGENT2SSH_'))}
 r=subprocess.run([str(x) for x in command],capture_output=True,text=True,timeout=timeout,env=env)
 if r.returncode:raise RuntimeError(f'{command}: exit={r.returncode}\n{r.stdout}\n{r.stderr}')
 return r


def details(path):
 info=run(['codesign','-dv','--verbose=4',path]).stderr
 found=re.search(r'^CDHash=([0-9a-f]{40})$',info,re.M)
 if not found:raise RuntimeError(f'no CDHash: {info}')
 return found.group(1),info


def port():
 with socket.socket() as s:
  s.bind(('127.0.0.1',0));return s.getsockname()[1]


def wait_ready(proc,addr):
 opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
 deadline=time.monotonic()+15
 while time.monotonic()<deadline:
  if proc.poll() is not None:raise RuntimeError(f'daemon exited before readiness: {proc.returncode}')
  try:
   with opener.open(f'http://127.0.0.1:{addr}/health',timeout=.5) as resp:
    if json.load(resp).get('pid')==proc.pid:return
  except (OSError,ValueError,urllib.error.URLError):pass
  time.sleep(.05)
 raise TimeoutError('daemon health readiness timed out')


source=args.daemon.resolve(strict=True)
with tempfile.TemporaryDirectory(prefix='a2s-real-daemon-',dir='/tmp') as td:
 root=Path(td);marker=root/'marker.dylib';attest=root/'attest'
 run(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-dynamiclib',ROOT/'injected_marker.c','-o',marker])
 run(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-O1',ROOT/'daemon_attest.c','-framework','Security','-framework','CoreFoundation','-lbsm','-o',attest])
 results=[]
 modes=[('ordinary',False,False,False),('runtime',True,False,False),
        ('runtime-dyld-only',True,True,False),('runtime-library-only',True,False,True),
        ('runtime-both',True,True,True)]
 for mode,hardened,allow_dyld,disable_library in modes:
  binary=root/f'daemon-{mode}';shutil.copy2(source,binary)
  sign=['codesign','--force','--sign','-','--identifier','org.agent2ssh.research.daemon']
  if hardened:sign+=['--options','runtime']
  if allow_dyld or disable_library:
   entitlements=root/'test-entitlements.plist'
   flags={}
   if allow_dyld:flags['com.apple.security.cs.allow-dyld-environment-variables']=True
   if disable_library:flags['com.apple.security.cs.disable-library-validation']=True
   entitlements.write_bytes(plistlib.dumps(flags))
   sign+=['--entitlements',entitlements]
  run(sign+[binary]);cdhash,info=details(binary)
  if 'Signature=adhoc' not in info or ('adhoc,runtime' in info)!=hardened:raise RuntimeError(f'unexpected signature: {info}')
  requirement=f'identifier "org.agent2ssh.research.daemon" and cdhash H"{cdhash}"'
  for injection in (False,True):
   for iteration in range(1,4):
    config=root/f'c{int(hardened)}{int(allow_dyld)}{int(disable_library)}{int(injection)}{iteration}';config.mkdir(mode=0o700)
    token=config/'daemon.token';token.write_text('synthetic-only-daemon-token');token.chmod(0o600)
    env={key:value for key,value in os.environ.items() if not key.startswith(('DYLD_','AGENT2SSH_'))}
    env['AGENT2SSH_CONFIG_DIR']=str(config);addr=port();env['AGENT2SSH_DAEMON_ADDR']=f'127.0.0.1:{addr}'
    env['AGENT2SSH_LOG']='error'
    if injection:env['DYLD_INSERT_LIBRARIES']=str(marker)
    with tempfile.TemporaryFile(mode='w+t') as log:
     proc=subprocess.Popen([str(binary)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
     try:
      wait_ready(proc,addr)
      observed=run([attest,config/'.daemon-ipc/s',str(proc.pid),requirement],timeout=8).stdout.strip()
      fields=re.search(r'peer_pid=(\d+) pidversion=(\d+) expected_pid=(\d+) code_status=(-?\d+) signing_info_status=(-?\d+) static_flags=(-?\d+) dynamic_status=(-?\d+) runtime_flag=(\d+) dyld_entitlement=(\d+) disable_library_validation=(\d+)',observed)
      if not fields or fields.group(1)!=str(proc.pid) or fields.group(3)!=str(proc.pid) or fields.group(4)!='0' or fields.group(5)!='0' or int(fields.group(7))<0 or fields.group(8)!=str(int(hardened)) or fields.group(9)!=str(int(allow_dyld)) or fields.group(10)!=str(int(disable_library)):
       raise AssertionError(f'unexpected audit/signing information: {observed}')
     finally:
      if proc.poll() is None:proc.terminate()
      try:proc.wait(timeout=8)
      except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
     log.seek(0);output=log.read()
    injected_count=output.count(MARKER)
    if proc.returncode!=0:raise RuntimeError(f'isolated daemon exit={proc.returncode}: {output}')
    expect_marker=injection and (not hardened or (allow_dyld and disable_library))
    if (expect_marker and injected_count<1) or (not expect_marker and injected_count!=0):
     raise AssertionError(f'unexpected marker count {injected_count}, runtime={hardened}, dyld={allow_dyld}, no_library_validation={disable_library}, injection={injection}')
    if (config/'daemon.pid').exists() or (config/'daemon.instance.json').exists() or (config/'.daemon-ipc/s').exists():
     raise AssertionError('isolated daemon left identity artifacts after SIGTERM')
    if details(binary)[0]!=cdhash:raise AssertionError('test binary code hash changed')
    line=f'runtime={int(hardened)} allow_dyld={int(allow_dyld)} no_library_validation={int(disable_library)} injected={int(injection)} run={iteration} marker_count={injected_count} {observed} exit={proc.returncode} records_removed=1\n'
    results.append(line);print(line,end='')
 if args.archive:(ROOT/'real-daemon-dyld-results.txt').write_text(''.join(results))
print('Verified 30 isolated real-daemon runs; synthetic token never sent by attestor')
