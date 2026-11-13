#!/usr/bin/env python3
"""Inspect an isolated ad-hoc Tauri bundle made from existing debug binaries.

Never installs or starts the app, signs only temporary copies and strips Apple
signing environment variables. The copied desktop binary can be stale; this
tests bundle signing policy, not the release artifact or runtime behavior.
"""
from pathlib import Path
import argparse
import os
import plistlib
import json
import re
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT=Path(__file__).resolve().parent
REPO=ROOT.parents[3]
parser=argparse.ArgumentParser()
parser.add_argument('--archive',action='store_true')
args=parser.parse_args()
env={k:v for k,v in os.environ.items() if not k.startswith(('APPLE_','TAURI_','DYLD_','AGENT2SSH_','CARGO_TARGET_DIR'))}
env['CI']='1'

def run(command,cwd,timeout=180):
 p=subprocess.run([str(x) for x in command],cwd=cwd,env=env,capture_output=True,text=True,timeout=timeout)
 if p.returncode:raise RuntimeError(f'{command}: exit={p.returncode}\n{p.stdout[-4000:]}\n{p.stderr[-4000:]}')
 return p

with tempfile.TemporaryDirectory(prefix='a2s-bundle-probe-',dir='/tmp') as td:
 temp=Path(td)
 archive=subprocess.Popen(['git','archive','HEAD'],cwd=REPO,env=env,stdout=subprocess.PIPE)
 extraction=subprocess.run(['tar','-x','-C',str(temp)],stdin=archive.stdout,capture_output=True,text=True,timeout=60)
 archive.stdout.close()
 if archive.wait(timeout=60) or extraction.returncode:raise RuntimeError(f'git archive extraction failed: {extraction.stderr}')
 (temp/'node_modules').symlink_to(REPO/'node_modules',target_is_directory=True)
 debug=temp/'src-tauri/target/debug';debug.mkdir(parents=True)
 shutil.copy2(REPO/'src-tauri/target/debug/agent2ssh-app',debug/'agent2ssh-app')
 triple=run(['rustc','-vV'],REPO).stdout.split('host: ',1)[1].splitlines()[0]
 binary_dir=temp/'src-tauri/binaries';binary_dir.mkdir(exist_ok=True)
 for source,dest in [('agent2ssh','agent2ssh-cli'),('agent2ssh-daemon','agent2ssh-daemon'),('agent2ssh-mcp','agent2ssh-mcp')]:
  binary=REPO/'src-tauri/target/debug'/source
  shutil.copy2(binary,binary_dir/f'{dest}-{triple}');shutil.copy2(binary,debug/source)
  if '-' in source:shutil.copy2(binary,debug/source.replace('-','_'))
 run([temp/'node_modules/.bin/tauri','bundle','--debug','--bundles','app','--ci'],temp,timeout=180)
 bundle=debug/'bundle/macos/Agent2SSH.app';application=bundle/'Contents/MacOS/agent2ssh-app';daemon=bundle/'Contents/MacOS/agent2ssh-daemon'
 if not application.is_file() or not daemon.is_file():raise RuntimeError('bundle did not contain expected binaries')
 lines=[]
 def inspect(label,path):
  signature=run(['codesign','-dv','--verbose=4',path],temp).stderr
  entitlements=run(['codesign','-d','--entitlements',':-',path],temp).stdout
  decoded=plistlib.loads(entitlements.encode()) if entitlements.strip() else {}
  relevant=['com.apple.security.cs.disable-library-validation','com.apple.security.cs.allow-unsigned-executable-memory','com.apple.security.cs.allow-dyld-environment-variables','com.apple.security.get-task-allow']
  fields=' '.join(f'{key.rsplit(".",1)[-1]}={int(decoded.get(key,False))}' for key in relevant)
  runtime=bool(re.search(r'^CodeDirectory .*flags=.*\bruntime\b',signature,re.MULTILINE))
  line=f'{label} ad_hoc={int("Signature=adhoc" in signature)} runtime={int(runtime)} {fields}\n';lines.append(line);print(line,end='')
  return decoded
 app_before=inspect('app_before',application);daemon_before=inspect('daemon_before',daemon)
 restricted=temp/'daemon-entitlements.plist';restricted.write_bytes(plistlib.dumps({}))
 run(['codesign','--force','--sign','-','--options','runtime','--entitlements',restricted,daemon],temp)
 run(['codesign','--force','--sign','-','--options','runtime','--entitlements',temp/'src-tauri/entitlements.plist',bundle],temp)
 run(['codesign','--verify','--deep','--strict',bundle],temp)
 app_after=inspect('app_after',application);daemon_after=inspect('daemon_after',daemon)
 if not daemon_before.get('com.apple.security.cs.disable-library-validation') or not daemon_before.get('com.apple.security.cs.allow-unsigned-executable-memory'):raise AssertionError('bundled daemon did not expose expected entitlements')
 if daemon_after.get('com.apple.security.cs.disable-library-validation') or daemon_after.get('com.apple.security.cs.allow-unsigned-executable-memory'):raise AssertionError('daemon remained permissive after re-signing')
 if app_after!=app_before:raise AssertionError('outer app entitlement changed unexpectedly')
 lines.append('restricted_resign deep_strict_verify=1\n');print(lines[-1],end='')
 # Run only the test-owned copied sidecar with an isolated config and token.
 # The attestor sends an anonymous hello and never reads that token.
 attest=temp/'attest';run(['clang','-std=c11','-D_DARWIN_C_SOURCE','-Wall','-Wextra','-Werror','-O1',ROOT/'daemon_attest.c','-framework','Security','-framework','CoreFoundation','-lbsm','-o',attest],temp)
 signature=run(['codesign','-dv','--verbose=4',daemon],temp).stderr
 cdhash=re.search(r'^CDHash=([0-9a-f]{40})$',signature,re.MULTILINE);identifier=re.search(r'^Identifier=(.+)$',signature,re.MULTILINE)
 if not cdhash or not identifier:raise AssertionError('restricted daemon signature lacks identity')
 requirement=f'identifier "{identifier.group(1)}" and cdhash H"{cdhash.group(1)}"'
 config=temp/'daemon-config';config.mkdir(mode=0o700);(config/'daemon.token').write_text('synthetic-only-daemon-token');(config/'daemon.token').chmod(0o600)
 with socket.socket() as listener:listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
 daemon_env=dict(env);daemon_env.update({'AGENT2SSH_CONFIG_DIR':str(config),'AGENT2SSH_DAEMON_ADDR':f'127.0.0.1:{port}','AGENT2SSH_LOG':'error'})
 with tempfile.TemporaryFile(mode='w+t') as log:
  proc=subprocess.Popen([str(daemon)],env=daemon_env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
  try:
   opener=urllib.request.build_opener(urllib.request.ProxyHandler({}));deadline=time.monotonic()+15;ready=False
   while time.monotonic()<deadline and proc.poll() is None:
    try:
     with opener.open(f'http://127.0.0.1:{port}/health',timeout=.5) as response:
      if json.load(response).get('pid')==proc.pid:ready=True;break
    except (OSError,ValueError,urllib.error.URLError):pass
    time.sleep(.05)
   if not ready:raise AssertionError('restricted bundled daemon did not become ready')
   observation=run([attest,config/'.daemon-ipc/s',str(proc.pid),requirement],temp).stdout
   if 'code_status=0 signing_info_status=0' not in observation or 'runtime_flag=1 dyld_entitlement=0 disable_library_validation=0' not in observation:raise AssertionError(f'unexpected running daemon signature: {observation}')
  finally:
   if proc.poll() is None:proc.terminate()
   try:proc.wait(timeout=8)
   except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
  if proc.returncode!=0 or any((config/p).exists() for p in ('daemon.pid','daemon.instance.json','.daemon-ipc/s')):raise AssertionError('restricted bundled daemon did not exit cleanly')
  line='restricted_daemon real_process_ready=1 dynamic_code_valid=1 runtime=1 library_validation_enabled=1 clean_exit=1\n';lines.append(line);print(line,end='')
 if args.archive:(ROOT/'bundle-results.txt').write_text(''.join(lines))
print('Inspected temporary ad-hoc debug bundle; desktop not launched or installed')
