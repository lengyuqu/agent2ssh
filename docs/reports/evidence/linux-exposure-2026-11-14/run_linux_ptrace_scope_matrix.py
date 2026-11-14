#!/usr/bin/env python3
"""CNB #28 — Linux /proc exposure vs. Yama kernel.yama.ptrace_scope.

Run as root inside a disposable *privileged* Linux container/VM whose kernel
has the Yama LSM. For each ptrace_scope value it records, per reader identity,
whether /proc/<pid>/cmdline and /proc/<pid>/environ of a stand-in target expose
the synthetic markers, plus a control: an actual PTRACE_ATTACH attempt, so a
"no change" row cannot be blamed on the knob being inert.

    python3 run_linux_ptrace_scope_matrix.py > raw-linux-ptrace-scope.txt

ptrace_scope is a host-wide sysctl (not namespaced). Scope 3 cannot be lowered
again until that kernel reboots, so it is only run with --include-scope-3.
"""
import json
import os
import platform
import subprocess
import sys
import time

ARGV_MARKER = "A2S_LINUX_ARGV_4D1A"
ENV_MARKER = "A2S_LINUX_ENV_C7E2"
TARGET_UID = 1001
OTHER_UID = 1002
YAMA = "/proc/sys/kernel/yama/ptrace_scope"

PROBE = r"""
import ctypes, json, os, sys
pid, argv_marker, env_marker = int(sys.argv[1]), sys.argv[2], sys.argv[3]
out = {}
def read(name, marker):
    try:
        with open(f"/proc/{pid}/{name}", "rb") as f:
            data = f.read()
        return "marker-visible" if marker.encode() in data else f"read-ok-no-marker(len={len(data)})"
    except OSError as e:
        return f"{type(e).__name__}(errno={e.errno})"
out["cmdline"] = read("cmdline", argv_marker)
out["environ"] = read("environ", env_marker)
libc = ctypes.CDLL(None, use_errno=True)
libc.ptrace.restype = ctypes.c_long
libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
PTRACE_ATTACH, PTRACE_DETACH = 16, 17
if libc.ptrace(PTRACE_ATTACH, pid, None, None) == 0:
    os.waitpid(pid, 0x40000000)  # __WALL: the target is not our child
    libc.ptrace(PTRACE_DETACH, pid, None, None)
    out["ptrace_attach"] = "attached"
else:
    out["ptrace_attach"] = f"refused(errno={ctypes.get_errno()})"
print(json.dumps(out))
"""


def sh(cmd, check=True):
    return subprocess.run(cmd, shell=True, check=check, capture_output=True, text=True)


def setpriv(uid):
    return ["setpriv", f"--reuid={uid}", f"--regid={uid}", "--clear-groups"]


READERS = [
    ("same-uid (not an ancestor)", setpriv(TARGET_UID)),
    ("other-uid", setpriv(OTHER_UID)),
    ("root-all-caps", []),
]


def main():
    if os.geteuid() != 0:
        sys.exit("run as root in a disposable Linux environment")
    if not os.path.exists(YAMA):
        sys.exit("this kernel has no Yama LSM; ptrace_scope cannot be measured here")
    scopes = ["0", "1", "2"] + (["3"] if "--include-scope-3" in sys.argv else [])
    original = open(YAMA).read().strip()
    print("## environment")
    print("kernel:", platform.release(), platform.machine())
    print("os:", sh("grep PRETTY_NAME /etc/os-release").stdout.strip())
    print("ptrace_scope at start:", original)
    print("lsm:", sh("cat /sys/kernel/security/lsm 2>/dev/null", check=False).stdout.strip() or "unavailable")
    print("proc mount:", sh("grep ' /proc ' /proc/self/mountinfo").stdout.split(" - ", 1)[1].strip())

    for uid in (TARGET_UID, OTHER_UID):
        sh(f"id -u a2s{uid} >/dev/null 2>&1 || useradd -M -u {uid} a2s{uid}")
    probe_path = "/tmp/a2s_ptrace_probe.py"
    with open(probe_path, "w") as f:
        f.write(PROBE)
    os.chmod(probe_path, 0o644)

    results = []
    reached = original
    try:
        for scope in scopes:
            with open(YAMA, "w") as f:
                f.write(scope)
            reached = open(YAMA).read().strip()
            # A fresh target per scope: a refused or detached attach must not
            # leak state into the next row.
            target = subprocess.Popen(
                setpriv(TARGET_UID) + ["perl", "-e", "sleep 600", ARGV_MARKER],
                env={"PATH": os.environ["PATH"], "A2S_SYNTH": ENV_MARKER},
            )
            time.sleep(0.5)
            print(f"\n## ptrace_scope={reached}   target pid={target.pid} uid={TARGET_UID}")
            for reader, prefix in READERS:
                run = subprocess.run(
                    prefix + ["python3", probe_path, str(target.pid), ARGV_MARKER, ENV_MARKER],
                    capture_output=True, text=True, env={"PATH": os.environ["PATH"]},
                )
                line = run.stdout.strip() or f"PROBE-FAILED exit={run.returncode} {run.stderr.strip()[:200]}"
                print(f"{reader:28s} {line}")
                try:
                    results.append({"ptrace_scope": reached, "reader": reader, **json.loads(line)})
                except ValueError:
                    results.append({"ptrace_scope": reached, "reader": reader, "error": line})
            target.kill()
            target.wait()
    finally:
        if reached != "3":
            with open(YAMA, "w") as f:
                f.write(original)
        for uid in (TARGET_UID, OTHER_UID):
            sh(f"userdel a2s{uid}", check=False)
        os.unlink(probe_path)
    print("\nptrace_scope at end:", open(YAMA).read().strip())
    print("\n## json")
    print(json.dumps(results, indent=1))


if __name__ == "__main__":
    main()
