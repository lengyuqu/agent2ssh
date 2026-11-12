#!/usr/bin/env python3
"""Which knob, if any, makes Windows PowerShell 5.1 stop prepending a UTF-8 BOM
to a native-command pipe? Each variant pipes the same ASCII marker into
`agent2ssh host add --password-stdin` and the stored bytes are printed."""

import os
import subprocess
import sys
import tempfile
import uuid

exe = os.path.abspath(sys.argv[1])
show = os.path.abspath(sys.argv[2])
PS51 = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"),
                    "System32", "WindowsPowerShell", "v1.0", "powershell.exe")

VARIANTS = {
    "default": "",
    "outputencoding-utf8-nobom": "$OutputEncoding = New-Object System.Text.UTF8Encoding $false; ",
    "outputencoding-utf8-bom": "$OutputEncoding = New-Object System.Text.UTF8Encoding $true; ",
    "console-outputencoding": "[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; ",
    "both": "$OutputEncoding = New-Object System.Text.UTF8Encoding $false; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false; ",
    "via-cmd": "",
}

for name, prefix in VARIANTS.items():
    cfg = os.path.join(os.environ.get("TEMP", "."), "a2s27-enc-" + uuid.uuid4().hex[:8])
    os.makedirs(cfg, exist_ok=True)
    env = os.environ.copy()
    env["AGENT2SSH_CONFIG_DIR"] = cfg
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env["AGENT2SSH_MASTER_PASSWORD"] = "probe-master-only"
    host = "enc-" + name
    if name == "via-cmd":
        script = ('"SYNTHETIC_ENC_CMD" | & cmd.exe /c "{}" host add {} --host 127.0.0.1 --password-stdin'
                  .format(exe, host))
        # cmd.exe is a child of 5.1 here; it forwards its own stdin unchanged.
    else:
        script = (prefix + '"SYNTHETIC_ENC_' + name.upper().replace("-", "_") +
                  '" | & "{}" host add {} --host 127.0.0.1 --password-stdin'.format(exe, host))
    proc = subprocess.run([PS51, "-NoLogo", "-NoProfile", "-Command", script],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    stored = subprocess.run([show, host], stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    hexed = ""
    for line in stored.stdout.decode("utf-8", "replace").splitlines():
        if line.startswith("hex:"):
            hexed = line[4:]
    print("{:<28} rc={} stored={}{}".format(
        name, proc.returncode, hexed,
        "" if hexed else " stderr=" + proc.stderr.decode("utf-8", "replace").strip()[:120]))
