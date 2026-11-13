#!/usr/bin/env python3
"""Transient, uniquely named user launchd/XPC jobs, always booted out.

Uses ad-hoc signatures and synthetic payloads. No user agent plist is installed,
no production daemon or credential is accessed. On cleanup failure, preserves
the temporary directory and prints the exact label for inspection.
"""
import os
import hashlib
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parent
DOMAIN = f"gui/{os.getuid()}"
SIGNING_ID = "org.agent2ssh.research.probe"


def run(args, timeout=15):
    return subprocess.run(args, text=True, capture_output=True, timeout=timeout)


def must(args):
    r = run(args)
    if r.returncode:
        raise RuntimeError(f"{args}: {r.returncode}: {r.stderr}")
    return r


def build(path, variant, identifier=SIGNING_ID):
    must(["clang", "-std=c11", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror", "-fblocks", "-O1", f'-DA2S_VARIANT="{variant}"', str(ROOT / "xpc_probe.c"), "-o", str(path)])
    must(["codesign", "--force", "--sign", "-", "--identifier", identifier, str(path)])


def service_state(label):
    try:
        result = run(["launchctl", "print", f"{DOMAIN}/{label}"])
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"
    if result.returncode == 0:
        return "present"
    if result.returncode == 113 and "Could not find service" in (result.stdout + result.stderr):
        return "absent"
    return "unknown"


def case(root, binary, requirement, tag, client_binary=None, client_requirement=None):
    label = f"org.agent2ssh.test.{os.getpid()}.{uuid.uuid4().hex[:12]}"
    plist = root / f"{tag}.plist"
    stdout = root / f"{tag}.out"
    stderr = root / f"{tag}.err"
    argv = [str(binary), "server", label]
    if client_requirement:
        argv.append(client_requirement)
    plist.write_bytes(plistlib.dumps({"Label": label, "ProgramArguments": argv, "MachServices": {label: True}, "StandardOutPath": str(stdout), "StandardErrorPath": str(stderr)}))
    installed = False
    try:
        result = run(["launchctl", "bootstrap", DOMAIN, str(plist)])
        print(f"{tag} bootstrap_exit={result.returncode}")
        if result.returncode:
            raise RuntimeError(f"bootstrap failed: {result.stderr}")
        installed = True
        client = run([str(client_binary or binary), "client", label, requirement], timeout=8)
        print(f"{tag} client_exit={client.returncode}")
        print(client.stdout.strip())
        time.sleep(0.15)
        observed = stdout.read_text() if stdout.exists() else ""
        print(f"{tag} server_received_synthetic={int('marker=synthetic-probe-only' in observed)}")
        if client_requirement:
            print(f"{tag} server_set_requirement={int('server_set_client_requirement_rc=0' in observed)}")
        return client.returncode, client.stdout, observed
    finally:
        if installed or service_state(label) != "absent":
            try:
                removed = run(["launchctl", "bootout", f"{DOMAIN}/{label}"])
                bootout_result = str(removed.returncode)
            except (OSError, subprocess.TimeoutExpired):
                bootout_result = "unavailable"
            deadline = time.monotonic() + 4
            while time.monotonic() < deadline and service_state(label) == "present":
                time.sleep(0.1)
            state = service_state(label)
            print(f"{tag} bootout_exit={bootout_result} final_service_state={state}")
            if state != "absent":
                raise RuntimeError(f"test service remains or cannot be verified: {DOMAIN}/{label}; files retained: {root}")


root = Path(tempfile.mkdtemp(prefix="a2s-xpc-", dir="/tmp"))
cleanup = True
try:
    a, b, good, bad = (root / name for name in ("server-a", "server-b", "client-good", "client-bad"))
    build(a, "original")
    build(b, "substitute")
    build(good, "client-good", "org.agent2ssh.research.client.good")
    build(bad, "client-bad", "org.agent2ssh.research.client.bad")
    details = run(["codesign", "-dv", "--verbose=4", str(a)]).stderr
    other_details = run(["codesign", "-dv", "--verbose=4", str(b)]).stderr
    if "Signature=adhoc" not in details or "TeamIdentifier=not set" not in details:
        raise AssertionError("expected ad-hoc server signature with no Team ID")
    different = hashlib.sha256(a.read_bytes()).digest() != hashlib.sha256(b.read_bytes()).digest()
    print(f"adhoc_no_team=1 different_binary_bytes={int(different)}")
    if not different: raise AssertionError("substitute must be a different binary")
    match_a = re.search(r"^CDHash=([0-9a-f]+)$", details, re.MULTILINE)
    match_b = re.search(r"^CDHash=([0-9a-f]+)$", other_details, re.MULTILINE)
    if not match_a or not match_b or match_a.group(1) == match_b.group(1):
        raise AssertionError("expected distinct code directory hashes")
    print("distinct_cdhash=1")
    # Both independent ad-hoc signed binaries have the same caller-selected ID.
    req = f'identifier "{SIGNING_ID}"'
    pinned = req + f' and cdhash H"{match_a.group(1)}"'
    tests = [
        ("match", a, req, good, None),
        ("wrong", a, 'identifier "org.agent2ssh.wrong"', good, None),
        ("same_id_substitute", b, req, good, None),
        ("apple_anchor", a, req + " and anchor apple generic", good, None),
        ("pinned_match", a, pinned, good, None),
        ("pinned_substitute", b, pinned, good, None),
        ("client_allowed", a, req, good, 'identifier "org.agent2ssh.research.client.good"'),
        ("client_rejected", a, req, bad, 'identifier "org.agent2ssh.research.client.good"'),
    ]
    for tag, binary, requirement, client_binary, client_requirement in tests:
        rc, output, seen = case(root, binary, requirement, tag, client_binary, client_requirement)
        if tag in ("wrong", "apple_anchor", "pinned_substitute", "client_rejected"):
            if rc == 0 or "result=error" not in output:
                raise AssertionError(f"requirement was not rejected in {tag}: {output}")
        elif rc != 0 or "result=reply" not in output or f"variant={'substitute' if tag == 'same_id_substitute' else 'original'}" not in output:
            raise AssertionError(f"unexpected result for {tag}: {output}")
        if tag == "client_rejected":
            if "server_set_client_requirement_rc=0" not in seen or "marker=synthetic-probe-only" in seen:
                raise AssertionError("server did not enforce the rejected client requirement")
        elif "marker=synthetic-probe-only" not in seen:
            raise AssertionError(f"server did not log synthetic request in {tag}")
        if tag == "client_allowed" and "server_set_client_requirement_rc=0" not in seen:
            raise AssertionError("server did not set the client requirement")
except Exception as error:
    if "test service remains" in str(error): cleanup = False
    print(f"ERROR: {error}")
    raise
finally:
    if cleanup: shutil.rmtree(root)
    else: print(f"temporary files retained at {root}")
