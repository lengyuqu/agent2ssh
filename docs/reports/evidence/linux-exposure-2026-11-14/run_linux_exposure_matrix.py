#!/usr/bin/env python3
"""CNB #28 — Linux /proc exposure matrix: hidepid x reader identity.

Run as root inside a disposable Linux container/VM that holds CAP_SYS_ADMIN
(needed to remount /proc). It never touches agent2ssh state: the target is a
stand-in process carrying two synthetic markers, one in argv and one in its
environment. Only marker presence is printed, never other environment content.

    python3 run_linux_exposure_matrix.py > raw-linux-exposure.txt

The script restores /proc to hidepid=0 and removes the users it created.
"""
import json
import os
import platform
import subprocess
import sys
import time

ARGV_MARKER = "A2S_LINUX_ARGV_4D1A"
ENV_MARKER = "A2S_LINUX_ENV_C7E2"
TARGET_UID = 1001  # owner of the target process
OTHER_UID = 1002  # unrelated account
GROUP_UID = 1003  # unrelated account that is a member of the hidepid gid= group
EXEMPT_GID = 2000

# The probe runs as each reader. It reports what that identity can observe
# about the target pid, as JSON on one line.
PROBE = r"""
import json, os, subprocess, sys
pid, argv_marker, env_marker = sys.argv[1], sys.argv[2], sys.argv[3]
out = {}
out["listed"] = pid in os.listdir("/proc")
def read(name, marker):
    try:
        with open(f"/proc/{pid}/{name}", "rb") as f:
            data = f.read()
        return "marker-visible" if marker.encode() in data else f"read-ok-no-marker(len={len(data)})"
    except OSError as e:
        return f"{type(e).__name__}(errno={e.errno})"
out["cmdline"] = read("cmdline", argv_marker)
out["environ"] = read("environ", env_marker)
ps = subprocess.run(["ps", "-p", pid, "-o", "args="], capture_output=True, text=True)
out["ps_args"] = "marker-visible" if argv_marker in ps.stdout else f"no-marker(exit={ps.returncode})"
print(json.dumps(out))
"""


def sh(cmd, check=True):
    return subprocess.run(cmd, shell=True, check=check, capture_output=True, text=True)


def proc_mount_options():
    with open("/proc/self/mountinfo") as f:
        for line in f:
            fields = line.split()
            if fields[4] == "/proc":
                return line.split(" - ", 1)[1].strip()
    return "?"


def setpriv(uid, gid, groups, extra=()):
    cmd = ["setpriv", f"--reuid={uid}", f"--regid={gid}"]
    cmd.append(f"--groups={','.join(map(str, groups))}" if groups else "--clear-groups")
    cmd.extend(extra)
    return cmd


READERS = [
    ("same-uid", setpriv(TARGET_UID, TARGET_UID, [])),
    ("other-uid", setpriv(OTHER_UID, OTHER_UID, [])),
    ("other-uid-in-gid-group", setpriv(GROUP_UID, GROUP_UID, [EXEMPT_GID])),
    ("root-all-caps", []),
    # Bounding-set drop takes effect across the exec that setpriv performs.
    ("root-without-CAP_SYS_PTRACE", ["setpriv", "--bounding-set=-sys_ptrace", "--inh-caps=-all"]),
]

MODES = [
    ("hidepid=0", "hidepid=0"),
    ("hidepid=1", "hidepid=1"),
    ("hidepid=2", "hidepid=2"),
    ("hidepid=2,gid=2000", f"hidepid=2,gid={EXEMPT_GID}"),
    ("hidepid=4 (ptraceable)", "hidepid=4"),
]


def main():
    if os.geteuid() != 0:
        sys.exit("run as root in a disposable Linux environment")
    print("## environment")
    print("kernel:", platform.release(), platform.machine())
    print("os:", sh("grep PRETTY_NAME /etc/os-release").stdout.strip())
    yama = "/proc/sys/kernel/yama/ptrace_scope"
    print("yama ptrace_scope:", open(yama).read().strip() if os.path.exists(yama) else "ABSENT (no Yama LSM in this kernel)")
    config = sh("zcat /proc/config.gz 2>/dev/null | grep -E 'CONFIG_SECURITY_YAMA|CONFIG_PROC_MEM'", check=False).stdout.strip()
    print("kernel config:", config.replace("\n", "; ") or "unavailable")
    print("proc mount at start:", proc_mount_options())
    print("root CapEff:", sh("grep CapEff /proc/self/status").stdout.split()[-1])

    for uid in (TARGET_UID, OTHER_UID, GROUP_UID):
        sh(f"id -u a2s{uid} >/dev/null 2>&1 || useradd -M -u {uid} a2s{uid}")
    sh(f"getent group {EXEMPT_GID} >/dev/null || groupadd -g {EXEMPT_GID} a2sproc")
    probe_path = "/tmp/a2s_probe.py"
    with open(probe_path, "w") as f:
        f.write(PROBE)
    os.chmod(probe_path, 0o644)

    env = {"PATH": os.environ["PATH"], "A2S_SYNTH": ENV_MARKER}
    target = subprocess.Popen(
        setpriv(TARGET_UID, TARGET_UID, []) + ["perl", "-e", "sleep 600", ARGV_MARKER], env=env
    )
    time.sleep(0.5)
    pid = str(target.pid)
    status = open(f"/proc/{pid}/status").read()
    uid_line = next(l for l in status.splitlines() if l.startswith("Uid:"))
    print(f"target: pid={pid} {uid_line.replace(chr(9), ' ')} argv marker={ARGV_MARKER} env marker={ENV_MARKER}")
    print()

    results = []
    try:
        for label, opts in MODES:
            # gid= persists across remounts unless reset explicitly.
            full = opts if "gid=" in opts else opts + ",gid=0"
            sh(f"mount -o remount,{full} /proc")
            print(f"## {label}   [mount: {proc_mount_options()}]")
            for reader, prefix in READERS:
                run = subprocess.run(
                    prefix + ["python3", probe_path, pid, ARGV_MARKER, ENV_MARKER],
                    capture_output=True, text=True, env={"PATH": os.environ["PATH"]},
                )
                line = run.stdout.strip() or f"PROBE-FAILED exit={run.returncode} {run.stderr.strip()[:200]}"
                print(f"{reader:30s} {line}")
                try:
                    results.append({"mode": label, "reader": reader, **json.loads(line)})
                except ValueError:
                    results.append({"mode": label, "reader": reader, "error": line})
            print()
    finally:
        sh("mount -o remount,hidepid=0,gid=0 /proc", check=False)
        target.kill()
        target.wait()
        for uid in (TARGET_UID, OTHER_UID, GROUP_UID):
            sh(f"userdel a2s{uid}", check=False)
        sh("groupdel a2sproc", check=False)
        os.unlink(probe_path)
    print("proc mount at end:", proc_mount_options())
    print()
    print("## json")
    print(json.dumps(results, indent=1))


if __name__ == "__main__":
    main()
