#!/usr/bin/env python3
"""Issue #21/#24 — what does the CLI say when OpenProcess is denied?

Runs the release probe next to this file:
  probe <agent2ssh.exe> <stand-in-pid>
The probe rewrites the stand-in's process DACL to SYSTEM + Administrators and
then launches the CLI through a restricted token (Administrators deny-only), so
`OpenProcess` answers ACCESS_DENIED for a process the same account started.

Synthetic only: a throwaway config dir under %TEMP%, `ping.exe` copied as
`agent2ssh-daemon.exe`, no real daemon, no real credential, nothing outside
that directory is written.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PROBE = os.path.join(HERE, "target", "release", "a2s-denied-cli.exe")
CLI = os.path.abspath(os.path.join(HERE, "..", "..", "..", "..", "src-tauri", "target", "debug", "agent2ssh.exe"))
SYSTEM_ROOT = os.environ.get("SystemRoot", r"C:\Windows")


def free_port():
    import socket

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    if not os.path.exists(PROBE):
        raise SystemExit(f"build the probe first: {PROBE}")
    if not os.path.exists(CLI):
        raise SystemExit(f"build the CLI first: {CLI}")

    workdir = tempfile.mkdtemp(prefix="a2s-denied-")
    config = os.path.join(workdir, "config")
    os.makedirs(config)
    with open(os.path.join(config, "daemon.token"), "w", encoding="utf-8") as handle:
        handle.write("synthetic-denied-probe-token")

    standin = os.path.join(workdir, "agent2ssh-daemon.exe")
    shutil.copyfile(os.path.join(SYSTEM_ROOT, "System32", "ping.exe"), standin)

    addr = f"127.0.0.1:{free_port()}"
    env = dict(os.environ)
    env["AGENT2SSH_CONFIG_DIR"] = config
    env["AGENT2SSH_DAEMON_ADDR"] = addr

    process = subprocess.Popen(
        [standin, "-t", "127.0.0.1"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=env,
    )
    try:
        time.sleep(0.4)
        with open(os.path.join(config, "daemon.pid"), "w", encoding="utf-8") as handle:
            handle.write(str(process.pid))

        result = {"config": config, "stand_in_pid": process.pid, "addr": addr}

        baseline = subprocess.run(
            [CLI, "daemon", "status", "--json"],
            capture_output=True,
            text=True,
            env=env,
            timeout=60,
        )
        try:
            result["elevated_status"] = json.loads(baseline.stdout)
        except json.JSONDecodeError:
            result["elevated_status_raw"] = baseline.stdout
            result["elevated_status_stderr"] = baseline.stderr

        probe = subprocess.run(
            [PROBE, CLI, str(process.pid)],
            capture_output=True,
            text=True,
            env=env,
            timeout=180,
        )
        result["probe_stdout"] = probe.stdout
        result["probe_stderr"] = probe.stderr
        result["probe_exit"] = probe.returncode
        result["stand_in_alive_after"] = process.poll() is None

        print(json.dumps(result, ensure_ascii=False, indent=2))
        with open(os.path.join(HERE, "raw-denied.json"), "w", encoding="utf-8") as handle:
            json.dump(result, handle, ensure_ascii=False, indent=2)
    finally:
        process.kill()
        process.wait()
        shutil.rmtree(workdir, ignore_errors=True)


if __name__ == "__main__":
    main()
