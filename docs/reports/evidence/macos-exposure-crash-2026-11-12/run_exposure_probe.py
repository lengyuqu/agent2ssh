#!/usr/bin/env python3
"""CNB issue #28: macOS cross-process argv/environ exposure measurement.

Runs entirely on synthetic markers in a throwaway child process. No credential
is read, written or displayed. Environment values are redacted in the archived
output: only variable NAMES are kept, plus the synthetic marker lines that
constitute the evidence.

Usage: python3 run_exposure_probe.py > raw-exposure.txt 2>&1
"""
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ARGV_MARKER = "A2S_EXPOSURE_ARGV_7F3C"
ENV_MARKER = "A2S_EXPOSURE_ENV_9B2E"
MARKERS = (ARGV_MARKER, ENV_MARKER)


def run(label, argv, env=None):
    print(f"\n$ {' '.join(str(a) for a in argv)}")
    p = subprocess.run(argv, capture_output=True, text=True, env=env)
    out = (p.stdout or "").rstrip()
    err = (p.stderr or "").rstrip()
    if out:
        print(out)
    if err:
        print("[stderr] " + err)
    print(f"[exit={p.returncode}] {label}")
    return out, p.returncode


def redacted_probe_output(raw):
    """Keep structure + marker lines, redact every other environment value."""
    kept = []
    for line in raw.splitlines():
        if line.startswith("S: "):
            if any(m in line for m in MARKERS):
                kept.append(line)  # the evidence itself
            elif "=" in line[3:]:
                kept.append("S: " + line[3:].split("=", 1)[0] + "=<redacted>")
            else:
                kept.append(line[:90])
        else:
            kept.append(line)
    return "\n".join(kept)


def run_probe(label, exe, pid):
    print(f"\n$ {exe} {pid}")
    p = subprocess.run([str(exe), str(pid)], capture_output=True, text=True)
    raw = p.stdout or ""
    print(redacted_probe_output(raw.rstrip()))
    if p.stderr:
        print("[stderr] " + p.stderr.rstrip())
    print(f"[exit={p.returncode}] {label}")
    return raw


def main():
    print("=== environment ===")
    run("os version", ["sw_vers"])
    run("sip", ["csrutil", "status"])
    run("identity", ["id"])
    run("kernel", ["uname", "-a"])
    run("hidepid equivalent", ["sh", "-c", "sysctl -a 2>/dev/null | grep -i hidepid || echo 'no hidepid sysctl on macOS'"])

    print("\n=== build probe ===")
    src = HERE / "procargs_probe.c"
    exe = HERE / "procargs_probe"
    run("clang", ["clang", "-O0", "-o", str(exe), str(src)])

    print("\n=== start target process (synthetic markers only) ===")
    child_env = dict(os.environ)
    child_env[ENV_MARKER] = "A2S_SYNTHETIC_VALUE_DO_NOT_USE"
    child_argv = [
        sys.executable,
        "-c",
        "import sys,time; print('child ready', flush=True); time.sleep(30)",
        ARGV_MARKER,
    ]
    print(f"$ env {ENV_MARKER}=<synthetic> {' '.join(child_argv)}")
    child = subprocess.Popen(child_argv, env=child_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    print(f"child_pid={child.pid} child_uid={os.getuid()}")
    time.sleep(0.5)

    print("\n=== E1: same-uid KERN_PROCARGS2 (argv + environ) ===")
    out = run_probe("read child procargs2", exe, child.pid)
    print(f"ARGV_MARKER_VISIBLE={ARGV_MARKER in out}")
    print(f"ENV_MARKER_VISIBLE={ENV_MARKER in out}")

    print("\n=== E2: ps argv visibility, same uid ===")
    ps_out, _ = run("ps -o args", ["ps", "-p", str(child.pid), "-o", "args="])
    print(f"PS_ARGV_MARKER_VISIBLE={ARGV_MARKER in ps_out}")
    all_ps = subprocess.run(["ps", "auxww"], capture_output=True, text=True).stdout
    hits = [ln for ln in all_ps.splitlines() if ARGV_MARKER in ln and "grep" not in ln]
    print(f"$ ps auxww | grep {ARGV_MARKER}  (only hit count is printed)\nPS_AUXWW_HIT_COUNT={len(hits)}")
    print(f"PS_AUXWW_MARKER_VISIBLE={len(hits) > 0}")

    print("\n=== E3: control - another same-uid process (parent shell) ===")
    parent_pid = os.getppid()
    run_probe(f"read same-uid pid {parent_pid} procargs2", exe, parent_pid)

    print("\n=== E4: different uid / pid 1 (launchd, SIP-protected) ===")
    out1 = run_probe("read pid 1 procargs2", exe, 1)
    print(f"PID1_MARKER_ABSENT={ARGV_MARKER not in out1}")
    run("ps -p 1 -o args", ["ps", "-p", "1", "-o", "args="])

    print("\n=== E5: other root-owned processes (different uid) ===")
    rc = subprocess.run(["ps", "-U", "root", "-o", "pid=", "-o", "comm="], capture_output=True, text=True)
    lines = [ln.strip() for ln in rc.stdout.splitlines() if ln.strip()]
    print(f"$ ps -U root -o pid= -o comm=  (first 3 of {len(lines)})")
    for ln in lines[:3]:
        print("  " + ln)
    for ln in lines[:2]:
        other_pid = ln.split()[0]
        run_probe(f"read root pid {other_pid} procargs2", exe, other_pid)

    print("\n=== E6: hidepid-like mount options on macOS ===")
    run("mount (first 6)", ["sh", "-c", "mount | head -6"])

    child.kill()
    child.wait()
    print("\n=== done ===")


if __name__ == "__main__":
    main()
