#!/usr/bin/env python3
"""Reproduce the recorded macOS observations using only test-owned children.

The guarded-port probe deliberately causes a test-owned child to be killed by
macOS when it attempts the forbidden transfer. A changed OS/security policy
can make an assertion fail; it must not be generalized to another platform.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent


def run(args, timeout=12):
    result = subprocess.run(args, text=True, capture_output=True, timeout=timeout)
    if result.returncode:
        raise AssertionError(f"{args}: exit={result.returncode}\n{result.stdout}\n{result.stderr}")
    return result.stdout


def need(output, *items):
    for item in items:
        if item not in output:
            raise AssertionError(f"missing {item!r}\n{output}")


def task_identity(binary, exec_transition):
    output = run([str(binary)] + (["--exec-transition"] if exec_transition else []))
    need(output, "dead_name_calibration kr=0 msg_id=72 expected=72 name_matches=1",
         "child_send_token kr=0", "parent_receive kr=0", "audit_equal=1",
         "live_convert flavor=0 kr=53", "live_convert flavor=1 kr=53",
         "live_convert flavor=2 kr=53", "live_convert flavor=3 kr=0",
         "task_audit_kr=0", "matches_sender=1", "dead_name_registration token_kr=0 name_kr=0",
         "name_notification_while_alive kr=268451843", "token_notification_after_exit kr=268451843")
    child = re.search(r"child_pid=(\d+) create_token kr=0", output)
    mach = re.search(r"mach_sender_pid=(\d+) sender_pidversion=(\d+) expected_child=(\d+)", output)
    sock = re.search(r"audit_pid=(\d+) audit_pidversion=(\d+) audit_equal=1", output)
    task = re.search(r"task_audit_pid=(\d+) task_audit_pidversion=(\d+) matches_sender=1", output)
    if not (child and mach and sock and task):
        raise AssertionError(f"missing identity fields\n{output}")
    pid = child.group(1)
    if not (mach.group(1) == mach.group(3) == sock.group(1) == task.group(1) == pid and mach.group(2) == sock.group(2) == task.group(2)):
        raise AssertionError(f"identity observations disagree\n{output}")
    if exec_transition:
        need(output, "name_notification_after_exec_while_alive kr=0 ((os/kern) successful) msg_id=72 name_matches=1",
             "exec_child_waitpid_nohang=0 still_running=1", "exec_old_token_convert_name kr=56", "equals_original=0")
        after = re.search(r"exec_socket_audit rc=0 pid=(\d+) pidversion=(\d+) equals_original=0", output)
        if not after or after.group(1) != pid or after.group(2) == mach.group(2):
            raise AssertionError(f"exec observation unexpected\n{output}")
    else:
        need(output, "name_notification_after_exit kr=0 ((os/kern) successful) msg_id=72 name_matches=1", "is_dead_name=1", "exited_convert flavor=3 kr=56")


def handoff(binary):
    output = run([str(binary)], 8)
    p = re.search(r"server_pid=(\d+) worker_pid=(\d+)", output)
    before = re.search(r"before_client_sends.*audit_pid=(\d+).*peer_pid=(\d+)", output)
    worker = re.search(r"worker_pid=(\d+) received_synthetic_byte=T", output)
    after = re.search(r"after_worker_read.*audit_pid=(\d+).*peer_pid=(\d+)", output)
    if not (p and before and worker and after and before.group(1) == before.group(2) == p.group(1) and after.group(1) == after.group(2) == worker.group(1) == p.group(2) and p.group(1) != p.group(2)):
        raise AssertionError(f"handoff observation unexpected\n{output}")


def guarded(binary):
    output = run([str(binary)], 8)
    need(output, "guarded=0 transfer_send_kr=0", "guarded=0 transfer_receive_kr=0",
         "guarded=1 guard_kr=0", "guarded=1 transfer_receive_kr=268451843",
         "guarded=1 child_exited=0 exit=-1 signaled=1 signal=9",
         "query_guarded=0 get_context_kr=17 receive_status_kr=17 flags=0x0 info_ext_kr=17",
         "query_guarded=1 get_context_kr=17 receive_status_kr=17 flags=0x0 info_ext_kr=17")


with tempfile.TemporaryDirectory(prefix="a2s-mach-verify-", dir="/tmp") as tmp:
    tmp = Path(tmp)
    for name, extra in [("probe", ["-lbsm"]), ("fd_handoff", ["-lbsm"]), ("guarded_receive", [])]:
        subprocess.run(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-O1", str(ROOT / f"{name}.c"), *extra, "-o", str(tmp / name)], check=True)
    for _ in range(3): task_identity(tmp / "probe", False)
    for _ in range(3): task_identity(tmp / "probe", True)
    for _ in range(5): handoff(tmp / "fd_handoff")
    for _ in range(3): guarded(tmp / "guarded_receive")
print("Verified: 3 direct, 3 exec, 5 handoff and 3 guarded-port runs")
