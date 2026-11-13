#!/usr/bin/env python3
"""Compare test-owned ad-hoc executable CDHash checks with dyld injection.

Only a harmless library writing a synthetic marker is injected. No production
daemon, user credential or persistent configuration is used.
"""
from pathlib import Path
import argparse
import os
import re
import signal
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
MARKER = "A2S_SYNTHETIC_INJECTED_LIBRARY"
parser = argparse.ArgumentParser()
parser.add_argument("--archive", action="store_true")
args = parser.parse_args()


def run(command, env=None, timeout=15):
    if env is None:
        env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
    p = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         env=env, start_new_session=True)
    try:
        out, err = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)  # only this invocation's probe process group
        p.communicate()
        raise
    if p.returncode:
        raise RuntimeError(f"failed ({p.returncode}): {command}\n{out}\n{err}")
    return out, err


with tempfile.TemporaryDirectory(prefix="a2s-audit-dyld-", dir="/tmp") as tmp:
    root = Path(tmp)
    library = root / "marker.dylib"
    run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-dynamiclib", str(ROOT / "injected_marker.c"), "-o", str(library)])
    archive = []
    for runtime in (False, True):
        binary = root / ("runtime" if runtime else "ordinary")
        run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-O1", str(ROOT / "audit_code.c"), "-framework", "Security", "-framework", "CoreFoundation", "-lbsm", "-o", str(binary)])
        sign = ["codesign", "--force", "--sign", "-", "--identifier", "org.agent2ssh.research.audit"]
        if runtime: sign += ["--options", "runtime"]
        run(sign + [str(binary)])
        _, info = run(["codesign", "-dv", "--verbose=4", str(binary)])
        match = re.search(r"^CDHash=([0-9a-f]{40})$", info, re.MULTILINE)
        if not match or "Signature=adhoc" not in info or ("adhoc,runtime" in info) != runtime:
            raise AssertionError(f"unexpected code signature: {info}")
        requirement = f'identifier "org.agent2ssh.research.audit" and cdhash H"{match.group(1)}"'
        for i in range(1, 4):
            for injected in (False, True):
                env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
                if injected: env["DYLD_INSERT_LIBRARIES"] = str(library)
                out, err = run([str(binary), requirement], env=env)
                count = out.count(MARKER)
                if not all(item in out for item in ("task_audit_match=1 correct_status=0", "new_audit_status=0", "waitpid_nohang=0")):
                    raise AssertionError(f"code/audit check failed:\n{out}\n{err}")
                if (not runtime and injected and count < 2) or ((runtime or not injected) and count != 0):
                    raise AssertionError(f"unexpected marker execution: runtime={runtime} injected={injected} count={count}\n{out}\n{err}")
                archive.append(f"runtime={int(runtime)} injected={int(injected)} run={i} marker_count={count} task_audit_match=1 code_check=0 post_exec_code_check=0\n")
    if args.archive: (ROOT / "dyld-results.txt").write_text("".join(archive))
    print("Verified 12 controlled runs: normal ad-hoc permitted the injected marker; ad-hoc runtime did not execute it")
