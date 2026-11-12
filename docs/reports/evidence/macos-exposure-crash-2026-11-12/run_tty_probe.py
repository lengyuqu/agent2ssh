#!/usr/bin/env python3
"""CNB issue #28: no-echo on a real PTY and no prompt on a non-TTY pipe.

A PTY has the real termios line discipline (kernel echo), so observing that
the synthetic password never appears in its output is a measurement of the
ECHO flag, not of a mocked reader. This is a pseudo-terminal, not Terminal.app
with ConPTY/conhost - that Windows-side scope belongs to #27.

Usage: python3 run_tty_probe.py > raw-tty.txt 2>&1
"""
import os
import pty
import select
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
CLI = REPO / "src-tauri/target/debug/agent2ssh"
MARKER = "A2S_TTY_MARKER_8D31"
FAILURES = []


def check(label, cond, detail=""):
    print(f"  [{'PASS' if cond else 'FAIL'}] {label}" + (f" :: {detail}" if detail else ""))
    if not cond:
        FAILURES.append(label)


def base_env(config):
    env = dict(os.environ)
    env["AGENT2SSH_CONFIG_DIR"] = str(config)
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    return env


def read_until(master_fd, needle, captured, timeout=15):
    import time
    deadline = time.time() + timeout
    target = needle if isinstance(needle, bytes) else needle.encode()
    while target not in captured:
        if time.time() > deadline:
            return False
        ready, _, _ = select.select([master_fd], [], [], 0.1)
        if not ready:
            continue
        try:
            chunk = os.read(master_fd, 4096)
        except OSError:
            return False
        if not chunk:
            return False
        captured.extend(chunk)
    return True


def pty_case(root):
    print("\n=== T1: real PTY, first-time set-password ===")
    config = root / "pty-config"
    config.mkdir()
    master_fd, slave_fd = pty.openpty()
    env = base_env(config)
    print(f"$ (pty) agent2ssh secrets set-password   config={config}")
    child = subprocess.Popen(
        [str(CLI), "secrets", "set-password"],
        stdin=slave_fd, stdout=slave_fd, stderr=slave_fd, env=env, close_fds=True,
    )
    os.close(slave_fd)
    captured = bytearray()

    ok1 = read_until(master_fd, b"New master password: ", captured)
    check("T1: first prompt observed on the PTY", ok1)
    # Include a multi-byte character and a backspace, as the existing unix test does.
    typed = (MARKER + "中\x7f\n").encode()
    print(f"  typed (hex, first 40 bytes)={typed[:40].hex()}  (synthetic marker, not a credential)")
    os.write(master_fd, typed)

    ok2 = read_until(master_fd, b"Confirm master password: ", captured)
    check("T1: confirmation prompt observed", ok2)
    os.write(master_fd, (MARKER + "\n").encode())

    ok3 = read_until(master_fd, b"Master password set.", captured)
    check("T1: success message observed", ok3)
    rc = child.wait(timeout=10)
    check("T1: process exited successfully", rc == 0, f"rc={rc}")
    os.close(master_fd)

    text = captured.decode("utf-8", "replace")
    check("T1: synthetic password never echoed on the PTY", MARKER not in text)
    print(f"  PTY output (escaped)={text[:300]!r}")
    check("T1: secrets.enc created", (config / "secrets.enc").exists())


def pipe_case(root):
    print("\n=== T2: non-TTY redirected stdin ===")
    config = root / "pipe-config"
    config.mkdir()
    env = base_env(config)
    print(f"$ printf '<synthetic>\\n' | agent2ssh secrets set-password   config={config}")
    p = subprocess.run(
        [str(CLI), "secrets", "set-password"], env=env,
        input=(MARKER + "\n").encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30,
    )
    print(f"  rc={p.returncode} stdout={p.stdout.decode(errors='replace').strip()!r}")
    print(f"  stderr={p.stderr.decode(errors='replace').strip()!r}")
    check("T2: pipe form succeeded", p.returncode == 0)
    check("T2: no interactive prompt printed to stderr", b"New master password:" not in p.stderr)
    check("T2: no interactive prompt printed to stdout", b"New master password:" not in p.stdout)
    check("T2: synthetic password absent from stdout/stderr",
          MARKER.encode() not in p.stdout and MARKER.encode() not in p.stderr)
    check("T2: secrets.enc created", (config / "secrets.enc").exists())


def main():
    print("=== environment ===")
    print(subprocess.run(["sw_vers"], capture_output=True, text=True).stdout.strip().replace("\n", " | "))
    print(f"  cli={CLI} exists={CLI.exists()}")
    if not CLI.exists():
        return 1
    root = Path(tempfile.mkdtemp(prefix="a2s-tty-"))
    print(f"  isolated root={root}  (never ~/.agent2ssh)")
    pty_case(root)
    pipe_case(root)
    print(f"\n=== summary === failures={len(FAILURES)}")
    for f in FAILURES:
        print(f"  FAILED: {f}")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
