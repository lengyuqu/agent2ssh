#!/usr/bin/env python3
"""Validate public audit-token code lookup on own ad-hoc signed children.

Does not access agent2ssh user data, real daemon processes or persistent jobs.
"""
from pathlib import Path
import argparse
import os
import re
import signal
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
parser = argparse.ArgumentParser()
parser.add_argument("--archive", action="store_true", help="replace results.txt with this run's synthetic observations")
args = parser.parse_args()


def run(command, timeout=15):
    env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True, env=env)
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        # This is the process group created for this exact test invocation.
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate()
        raise
    result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
    if result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}): {command}\n{result.stdout}\n{result.stderr}")
    return result


with tempfile.TemporaryDirectory(prefix="a2s-audit-code-", dir="/tmp") as temp:
    binary = Path(temp) / "audit-code"
    run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-O1", str(ROOT / "audit_code.c"), "-framework", "Security", "-framework", "CoreFoundation", "-lbsm", "-o", str(binary)])
    run(["codesign", "--force", "--sign", "-", "--identifier", "org.agent2ssh.research.audit", str(binary)])
    info = run(["codesign", "-dv", "--verbose=4", str(binary)]).stderr
    match = re.search(r"^CDHash=([0-9a-f]{40})$", info, re.MULTILINE)
    if not match or "Signature=adhoc" not in info or "TeamIdentifier=not set" not in info:
        raise AssertionError(f"expected ad-hoc signature without Team ID:\n{info}")
    requirement = f'identifier "org.agent2ssh.research.audit" and cdhash H"{match.group(1)}"'
    replacement = Path(temp) / "audit-code-replacement"
    run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", '-DA2S_VARIANT="replacement"', "-Wall", "-Wextra", "-Werror", "-O1", str(ROOT / "audit_code.c"), "-framework", "Security", "-framework", "CoreFoundation", "-lbsm", "-o", str(replacement)])
    run(["codesign", "--force", "--sign", "-", "--identifier", "org.agent2ssh.research.audit", str(replacement)])
    replacement_info = run(["codesign", "-dv", "--verbose=4", str(replacement)]).stderr
    replacement_hash = re.search(r"^CDHash=([0-9a-f]{40})$", replacement_info, re.MULTILINE)
    if not replacement_hash or replacement_hash.group(1) == match.group(1) or "Signature=adhoc" not in replacement_info:
        raise AssertionError(f"replacement must have a different ad-hoc CDHash:\n{replacement_info}")
    replacement_requirement = f'identifier "org.agent2ssh.research.audit" and cdhash H"{replacement_hash.group(1)}"'
    handoff = Path(temp) / "mach-handoff"
    run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-O1", str(ROOT / "mach_handoff.c"), "-framework", "Security", "-framework", "CoreFoundation", "-lbsm", "-o", str(handoff)])
    run(["codesign", "--force", "--sign", "-", "--identifier", "org.agent2ssh.research.handoff", str(handoff)])
    handoff_info = run(["codesign", "-dv", "--verbose=4", str(handoff)]).stderr
    handoff_hash = re.search(r"^CDHash=([0-9a-f]{40})$", handoff_info, re.MULTILINE)
    if not handoff_hash or "Signature=adhoc" not in handoff_info:
        raise AssertionError(f"expected ad-hoc signed handoff executable:\n{handoff_info}")
    handoff_requirement = f'identifier "org.agent2ssh.research.handoff" and cdhash H"{handoff_hash.group(1)}"'
    runs = []
    for i in range(1, 6):
        result = run([str(binary), requirement])
        first = re.search(r"child_pid=(\d+) sender_pid=(\d+) pidversion=(\d+) task_audit_match=(\d+) correct_status=(-?\d+) wrong_status=(-?\d+) wrong_hash_status=(-?\d+)", result.stdout)
        after = re.search(r"after_exec_pid=(\d+) pidversion=(\d+) old_audit_status=(-?\d+) new_audit_status=(-?\d+) waitpid_nohang=(-?\d+)", result.stdout)
        retained = re.search(r"retained_code_after_exec_status=(-?\d+)", result.stdout)
        notification = re.search(r"after_exec_task_notification_kr=(\d+) notification_is_dead_name=(\d+) name_task_info_kr=(\d+)", result.stdout)
        reopen = re.search(r"after_exec_original_identity_reopen_kr=(\d+)", result.stdout)
        if not first or not after or not retained or not notification or not reopen or 'post_register_task_audit_match=1' not in result.stdout or 'proc_events_after_exec=0' not in result.stdout or 'proc_events_after_exit=1 exit_flag=1 event_pid_matches=1' not in result.stdout:
            raise AssertionError(f"missing observation:\n{result.stdout}")
        if not (first.group(1) == first.group(2) == after.group(1) and first.group(3) != after.group(2)
                and first.group(4) == "1" and first.group(5) == "0" and first.group(6) != "0"
                and first.group(7) != "0" and after.group(3) != "0" and after.group(4) == "0" and after.group(5) == "0" and retained.group(1) != "0"
                and notification.group(1) == "0" and notification.group(2) == "1" and notification.group(3) != "0" and reopen.group(1) != "0"):
            raise AssertionError(f"unexpected audit/code result:\n{result.stdout}")
        runs.append(f"run {i} exit=0\n{result.stdout}")
    replaced_runs = []
    for i in range(1, 4):
        result = run([str(binary), requirement, str(replacement), replacement_requirement])
        first = re.search(r"child_pid=(\d+) sender_pid=(\d+) pidversion=(\d+) task_audit_match=(\d+) correct_status=(-?\d+) wrong_status=(-?\d+) wrong_hash_status=(-?\d+)", result.stdout)
        after = re.search(r"after_exec_pid=(\d+) pidversion=(\d+) old_audit_status=(-?\d+) new_audit_status=(-?\d+) waitpid_nohang=(-?\d+) replacement_status=(-?\d+) replacement_mode=(\d+)", result.stdout)
        retained = re.search(r"retained_code_after_exec_status=(-?\d+)", result.stdout)
        notification = re.search(r"after_exec_task_notification_kr=(\d+) notification_is_dead_name=(\d+) name_task_info_kr=(\d+)", result.stdout)
        reopen = re.search(r"after_exec_original_identity_reopen_kr=(\d+)", result.stdout)
        if not first or not after or not retained or not notification or not reopen or 'post_register_task_audit_match=1' not in result.stdout or 'proc_events_after_exec=0' not in result.stdout or 'proc_events_after_exit=1 exit_flag=1 event_pid_matches=1' not in result.stdout or not (first.group(1) == first.group(2) == after.group(1) and first.group(3) != after.group(2)
                and first.group(4) == "1" and first.group(5) == "0" and first.group(6) != "0" and first.group(7) != "0"
                and after.group(3) != "0" and after.group(4) != "0" and after.group(5) == "0" and after.group(6) == "0" and after.group(7) == "1" and retained.group(1) != "0"
                and notification.group(1) == "0" and notification.group(2) == "1" and notification.group(3) != "0" and reopen.group(1) != "0"):
            raise AssertionError(f"unexpected replacement process result:\n{result.stdout}")
        replaced_runs.append(f"run {i} exit=0\n{result.stdout}")
    handoff_runs = []
    for i in range(1, 6):
        result = run([str(handoff), handoff_requirement])
        owner = re.search(r"verified_owner_pid=(\d+) sender_pid=(\d+) task_audit_match=(\d+) pinned_code_status=(-?\d+)", result.stdout)
        worker = re.search(r"worker_received_synthetic pid=(\d+) marker=0x51a2", result.stdout)
        if not owner or not worker or not (owner.group(1) == owner.group(2) and owner.group(1) != worker.group(1) and owner.group(3) == "1" and owner.group(4) == "0" and "after_handoff_owner_alive=1 worker_alive=1" in result.stdout):
            raise AssertionError(f"unexpected authenticated Mach handoff result:\n{result.stdout}")
        handoff_runs.append(f"run {i} exit=0\n{result.stdout}")
    guarded_handoff_runs = []
    for i in range(1, 4):
        result = run([str(handoff), handoff_requirement, "--guarded"])
        owner = re.search(r"verified_owner_pid=(\d+) sender_pid=(\d+) task_audit_match=(\d+) pinned_code_status=(-?\d+)", result.stdout)
        worker = re.search(r"worker_cancelled pid=(\d+)", result.stdout)
        if not owner or not worker or not (owner.group(1) == owner.group(2) and owner.group(1) != worker.group(1) and owner.group(3) == "1" and owner.group(4) == "0" and "guard_kr=0" in result.stdout and "guarded_transfer_receive_kr=268451843 owner_signaled=1 owner_signal=9 worker_received_synthetic=0" in result.stdout and "worker_received_synthetic pid=" not in result.stdout):
            raise AssertionError(f"unexpected guarded Mach handoff result:\n{result.stdout}")
        guarded_handoff_runs.append(f"run {i} exit=0\n{result.stdout}")
    if args.archive:
        (ROOT / "results.txt").write_text("\n".join(runs))
        (ROOT / "replacement-results.txt").write_text("\n".join(replaced_runs))
        (ROOT / "mach-handoff-results.txt").write_text("\n".join(handoff_runs))
        (ROOT / "mach-guarded-handoff-results.txt").write_text("\n".join(guarded_handoff_runs))
    print("Verified 5 same-code, 3 replaced-code, 5 handoff and 3 guarded-handoff runs; ad-hoc signatures, no Team ID")
