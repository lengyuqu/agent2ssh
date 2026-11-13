#!/usr/bin/env python3
"""Reproduce ephemeral XPC jobs and per-message Mach identity experiments.

Uses only ad-hoc signatures, synthetic messages and test-owned child processes.
Each XPC run creates eight uniquely named user launchd jobs and bootouts them.
See run_xpc_probe.py for its cleanup and failure handling.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent


def run(args, timeout):
    r = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    if r.returncode:
        raise RuntimeError(f"{args} failed ({r.returncode}):\n{r.stdout}\n{r.stderr}")
    return r.stdout


def require(text, marker):
    if marker not in text:
        raise AssertionError(f"missing {marker!r}:\n{text}")


with tempfile.TemporaryDirectory(prefix="a2s-mach-msg-", dir="/tmp") as tmp:
    binary = Path(tmp) / "mach_per_message"
    subprocess.run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-O1", str(ROOT / "mach_per_message.c"), "-lbsm", "-o", str(binary)], check=True)
    for _ in range(3):
        output = run([str(binary)], 8)
        rows = re.findall(r"message=(\d+) expected_pid=(\d+) actual_pid=(\d+) pidversion=(\d+) task_matches_trailer=1", output)
        if len(rows) != 2 or rows[0][0] != "1" or rows[1][0] != "2" or any(row[1] != row[2] for row in rows) or rows[0][1] == rows[1][1]:
            raise AssertionError(f"per-message identity mismatch:\n{output}")
        require(output, "pinned_first_accept_second=0")
    for _ in range(3):
        # The child bounds its launchctl/client calls and owns cleanup; do not
        # kill it with an outer timeout while a temporary job might be live.
        output = run(["python3", str(ROOT / "run_xpc_probe.py")], None)
        for marker in ("adhoc_no_team=1 different_binary_bytes=1", "distinct_cdhash=1", "match client_exit=0", "wrong client_exit=5", "wrong server_received_synthetic=1", "same_id_substitute client_exit=0", "apple_anchor client_exit=5", "apple_anchor server_received_synthetic=1", "pinned_match client_exit=0", "pinned_substitute client_exit=5", "pinned_substitute server_received_synthetic=1", "client_allowed client_exit=0", "client_allowed server_set_requirement=1", "client_rejected client_exit=5", "client_rejected server_received_synthetic=0", "client_rejected server_set_requirement=1"):
            require(output, marker)
        if output.count("final_service_state=absent") != 8:
            raise AssertionError(f"not all temporary services confirmed absent:\n{output}")
print("Verified: 3 per-message Mach runs and 3 XPC rounds; all 24 jobs removed")
