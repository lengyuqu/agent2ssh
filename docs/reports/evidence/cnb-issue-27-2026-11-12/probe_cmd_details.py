#!/usr/bin/env python3
"""Diagnostics for the two cmd.exe questions left open by the first pass:
how `type` handles a non-UTF-8 byte in a pipe, and the exact quoting shape a
cmd redirection needs when the executable path is quoted."""

import os
import subprocess
import sys
import tempfile

exe = os.path.abspath(sys.argv[1])
tmp = tempfile.mkdtemp(prefix="a2s27-diag-")

bad = os.path.join(tmp, "bad.bin")
# 0xC3 0x28 is an invalid UTF-8 sequence but not a UTF-16 BOM: `type` emits a
# file that starts with FF FE as nothing at all (measured below), which would
# test cmd rather than the CLI.
with open(bad, "wb") as handle:
    handle.write(b"\xc3\x28")

# 1. What does `type` emit for those bytes on its own?
out = subprocess.run(
    "cmd.exe /c type " + '"' + bad + '"',
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)
print("type-stdout-hex:", out.stdout.hex())
print("type-stderr:", out.stderr.decode("utf-8", "replace").strip())
print("type-rc:", out.returncode)

# 1b. The UTF-16-BOM file that produced an empty pipe in the first pass.
bom = os.path.join(tmp, "bom.bin")
with open(bom, "wb") as handle:
    handle.write(b"\xff\xfe\x41")
out_bom = subprocess.run(
    "cmd.exe /c type " + '"' + bom + '"',
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
)
print("type-bom-stdout-hex:", out_bom.stdout.hex())
print("type-bom-rc:", out_bom.returncode)

# 3. Redirect quoting: single-quoted exe path plus a quoted file, wrapped in
# one more pair of quotes so cmd's `/c` heuristic keeps the command intact.
secret = os.path.join(tmp, "secret.txt")
with open(secret, "wb") as handle:
    handle.write(b"SYNTHETIC_CMD_REDIRECT\r\n")
env = os.environ.copy()
env["AGENT2SSH_CONFIG_DIR"] = os.path.join(tmp, "cfg")
env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
env["AGENT2SSH_MASTER_PASSWORD"] = "probe-master-only"
os.makedirs(env["AGENT2SSH_CONFIG_DIR"], exist_ok=True)

cmd = '""' + exe + '" host add redir --host 127.0.0.1 --password-stdin --json < "' + secret + '""'
proc = subprocess.run('cmd.exe /c ' + cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
print("redirect-rc:", proc.returncode)
print("redirect-stderr:", proc.stderr.decode("utf-8", "replace").strip())
print("redirect-stdout:", proc.stdout.decode("utf-8", "replace").strip()[:200])

show = os.path.join(os.path.dirname(exe), "examples", "show_secret.exe")
secret_out = subprocess.run([show, "redir"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
print("redirect-secret:", secret_out.stdout.decode("utf-8", "replace").replace("\n", " | "))
print("redirect-secret-rc:", secret_out.returncode, secret_out.stderr.decode("utf-8", "replace").strip())
print("tmp:", tmp)
