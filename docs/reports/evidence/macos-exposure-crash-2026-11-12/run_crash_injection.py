#!/usr/bin/env python3
"""CNB issue #28: real SIGKILL injection into the two-file commit.

The real `agent2ssh` CLI process runs `host add --password-stdin` with an
external DYLD-interpose library loaded. At a chosen point the library pauses
and drops a marker; this script sends SIGKILL from outside the process. No
production code is modified, no real credential is used, and every run uses a
throwaway AGENT2SSH_CONFIG_DIR (never ~/.agent2ssh).

Cases:
  before-rename  crash after store_commit::begin(), before the hosts rename
  before-finish  crash after the hosts rename, before finish() removes the log
  lost-tmp       before-rename crash + delete the prepared temp file, then
                 restart: exercises the "cannot be replayed" branch

Usage: python3 run_crash_injection.py > raw-crash.txt 2>&1
"""
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
CLI = REPO / "src-tauri/target/debug/agent2ssh"
HOOK_SRC = REPO / "src-tauri/tests/fixtures/crash_hook.c"
MASTER = "txn-crash-master-only"
SEED_MARKER = "A2S_CRASH_SEED_MARKER_41A7"
NEW_MARKER = "A2S_CRASH_NEW_MARKER_6C29"
MARKERS = (SEED_MARKER, NEW_MARKER)

FAILURES = []


def check(label, cond, detail=""):
    print(f"  [{'PASS' if cond else 'FAIL'}] {label}" + (f" :: {detail}" if detail else ""))
    if not cond:
        FAILURES.append(label)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


def no_marker_bytes(data, where):
    text = data.decode("utf-8", "replace")
    for marker in MARKERS:
        check(f"no {marker[:12]} leaked into {where}", marker not in text)


def base_env(config, master=True, hook=None, hook_dir=None):
    env = dict(os.environ)
    env["AGENT2SSH_CONFIG_DIR"] = str(config)
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    env.pop("DYLD_INSERT_LIBRARIES", None)
    env.pop("A2S_HOOK_STAGE", None)
    env.pop("A2S_HOOK_DIR", None)
    if master:
        env["AGENT2SSH_MASTER_PASSWORD"] = MASTER
    if hook:
        env["DYLD_INSERT_LIBRARIES"] = str(DYLIB)
        env["A2S_HOOK_STAGE"] = hook
        env["A2S_HOOK_DIR"] = str(hook_dir)
    return env


def run_cli(config, args, stdin=None, master=True, hook=None, hook_dir=None):
    env = base_env(config, master=master, hook=hook, hook_dir=hook_dir)
    return subprocess.run(
        [str(CLI), *args], env=env, input=stdin,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30,
    )


def seed(root):
    config = root / "seed"
    config.mkdir(parents=True)
    print("\n=== seed: initialize store + one sealed credential ===")
    p = run_cli(config, ["secrets", "set-password"], stdin=(MASTER + "\n").encode(), master=False)
    print(f"$ secrets set-password  -> {p.returncode} stderr={p.stderr.decode(errors='replace').strip()!r}")
    check("master password set", p.returncode == 0)
    p = run_cli(config, ["host", "add", "seed", "--host", "127.0.0.1", "--password-stdin"], stdin=SEED_MARKER.encode())
    print(f"$ host add seed --password-stdin -> {p.returncode} stdout={p.stdout.decode(errors='replace').strip()!r}")
    check("seed host added", p.returncode == 0)
    no_marker_bytes(p.stdout, "seed stdout")
    no_marker_bytes(p.stderr, "seed stderr")
    for f in config.iterdir():
        if f.is_file():
            no_marker_bytes(f.read_bytes(), f"seed file {f.name}")
    return config


def snapshot(config, label):
    print(f"\n--- {label}: on-disk state ---")
    files = sorted(p for p in config.iterdir() if p.is_file())
    for f in files:
        print(f"  {f.name:42s} {f.stat().st_size:8d}  sha256={(sha(f) or '')[:16]}")
    hosts_path, secrets_path, log_path = config / "hosts.json", config / "secrets.enc", config / ".commit.json"
    hosts = json.loads(hosts_path.read_text())
    secrets = json.loads(secrets_path.read_text())
    log = json.loads(log_path.read_text()) if log_path.exists() else None
    print(f"  hosts.secrets_generation={hosts.get('secrets_generation')} secrets.generation={secrets.get('generation')}")
    print(f"  host names={[h.get('name') for h in hosts.get('hosts', [])]}")
    print(f"  secret keys={sorted(secrets.get('entries', {}).keys())}")
    print(f"  commit log={json.dumps(log, sort_keys=True) if log else None}")
    return hosts, secrets, log, files


def assert_consistent(hosts, secrets, label):
    check(f"{label}: generation aligned", hosts.get("secrets_generation") == secrets.get("generation"),
          f"{hosts.get('secrets_generation')} vs {secrets.get('generation')}")
    entries = secrets.get("entries", {})
    refs = []
    for host in hosts.get("hosts", []):
        for field in ("password", "passphrase"):
            if isinstance(host.get(field), str) and host[field].startswith("$agent2ssh-secret$"):
                refs.append((host.get("name"), "host:" + str(host.get("name")), field))
    for name, key, field in refs:
        check(f"{label}: reference for {name}.{field} has a ciphertext entry", key in entries, key)
    print(f"  NOTE: with the v2 bare marker, key existence + generation alignment is all that can be "
          f"verified without decrypting; content matching requires #26 phase 2 tags.")


def crash_case(root, seed_config, stage, hook_dylib, tag=None):
    global DYLIB
    DYLIB = hook_dylib
    case = root / f"case-{tag or stage}"
    config = case / "config"
    hook_dir = case / "hook"
    shutil.copytree(seed_config, config)
    hook_dir.mkdir(parents=True)
    print(f"\n=========== CASE {stage} ===========")
    print(f"$ DYLD_INSERT_LIBRARIES=<hook> A2S_HOOK_STAGE={stage} "
          f"agent2ssh host add crash-target --host 127.0.0.2 --password-stdin")

    env = base_env(config, hook=stage, hook_dir=hook_dir)
    child = subprocess.Popen(
        [str(CLI), "host", "add", "crash-target", "--host", "127.0.0.2", "--password-stdin"],
        env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    child.stdin.write(NEW_MARKER.encode())
    child.stdin.close()
    fired = False
    deadline = time.time() + 20
    while time.time() < deadline:
        if (hook_dir / f"{stage}.fired").exists():
            fired = True
            break
        if child.poll() is not None:
            break
        time.sleep(0.01)
    check(f"{stage}: hook reached the intended breakpoint", fired,
          f"child_status={child.poll()}")
    if fired:
        os.kill(child.pid, signal.SIGKILL)
    out, err = child.communicate(timeout=10)
    print(f"  child returncode={child.returncode} (SIGKILL = -9)")
    check(f"{stage}: process died from SIGKILL", child.returncode == -9)
    no_marker_bytes(out, f"{stage} stdout")
    no_marker_bytes(err, f"{stage} stderr")

    hosts, secrets, log, files = snapshot(config, f"{stage} after crash")
    for f in files:
        no_marker_bytes(f.read_bytes(), f"{stage} file {f.name}")
    check(f"{stage}: commit log survived the crash", log is not None)
    tmp_name = log.get("tmp_name") if log else None
    tmp_path = config / tmp_name if tmp_name else None
    has_target = any(h.get("name") == "crash-target" for h in hosts.get("hosts", []))
    print(f"  tmp_name={tmp_name} tmp_exists={tmp_path.exists() if tmp_path else None} hosts_has_crash_target={has_target}")
    if stage == "before-rename":
        check("before-rename: hosts.json still holds the previous config", not has_target)
        check("before-rename: prepared temp file is intact", bool(tmp_path and tmp_path.exists()))
        check("before-rename: tmp hash matches the logged target hash",
              bool(tmp_path and tmp_path.exists() and sha(tmp_path) == log.get("hosts_sha256")))
    elif stage == "before-finish":
        check("before-finish: hosts rename already completed", has_target)
        check("before-finish: hosts hash matches the logged target hash",
              sha(config / "hosts.json") == log.get("hosts_sha256"))
        check("before-finish: prepared temp file was consumed", not (tmp_path and tmp_path.exists()))
    check(f"{stage}: logged secrets hash matches secrets.enc",
          log and sha(config / "secrets.enc") == log.get("secrets_sha256"))
    return config


def restart(config, label, expect_success=True, extra_name="after-recovery"):
    print(f"\n--- {label}: restart (fresh CLI process, no hook) ---")
    p = run_cli(config, ["host", "add", extra_name, "--host", "127.0.0.3"])
    print(f"$ host add {extra_name} --host 127.0.0.3 -> {p.returncode}")
    print(f"  stdout={p.stdout.decode(errors='replace').strip()!r}")
    print(f"  stderr={p.stderr.decode(errors='replace').strip()!r}")
    check(f"{label}: restart write {'succeeded' if expect_success else 'was refused'}",
          (p.returncode == 0) == expect_success)
    no_marker_bytes(p.stdout, f"{label} stdout")
    no_marker_bytes(p.stderr, f"{label} stderr")
    hosts, secrets, log, files = snapshot(config, f"{label} after restart")
    for f in files:
        no_marker_bytes(f.read_bytes(), f"{label} file {f.name}")
    return hosts, secrets, log, p


def main():
    print("=== toolchain / host ===")
    print(f"  repo={REPO}")
    print(f"  cli={CLI} exists={CLI.exists()}")
    run_out = subprocess.run(["sw_vers"], capture_output=True, text=True)
    print("  " + run_out.stdout.strip().replace("\n", " | "))
    if not CLI.exists():
        print("FAIL: build the CLI first: cargo build --no-default-features --bin agent2ssh")
        return 1

    root = Path(tempfile.mkdtemp(prefix="a2s-crash-"))
    print(f"  isolated root={root}  (never ~/.agent2ssh)")
    try:
        dylib = root / "liba2s_crashhook.dylib"
        print(f"\n$ clang -dynamiclib -o {dylib} {HOOK_SRC}")
        b = subprocess.run(["clang", "-dynamiclib", "-o", str(dylib), str(HOOK_SRC)], capture_output=True, text=True)
        check("hook dylib built", b.returncode == 0, b.stderr.strip())
        if b.returncode != 0:
            return 1

        seed_config = seed(root)

        # Case 1: after begin(), before rename(tmp, hosts.json)
        c1 = crash_case(root, seed_config, "before-rename", dylib)
        hosts, secrets, log, _ = restart(c1, "before-rename")
        check("before-rename: recovery replayed the prepared hosts file",
              any(h.get("name") == "crash-target" for h in hosts.get("hosts", [])))
        check("before-rename: restart write built on the replayed config",
              any(h.get("name") == "after-recovery" for h in hosts.get("hosts", [])))
        check("before-rename: commit log cleared", log is None)
        assert_consistent(hosts, secrets, "before-rename")

        # Case 2: after the hosts rename, before finish() removes the log
        c2 = crash_case(root, seed_config, "before-finish", dylib)
        hosts, secrets, log, _ = restart(c2, "before-finish")
        check("before-finish: hosts already holds the committed config",
              any(h.get("name") == "crash-target" for h in hosts.get("hosts", [])))
        check("before-finish: no double replay / no data loss",
              any(h.get("name") == "after-recovery" for h in hosts.get("hosts", [])))
        check("before-finish: leftover log cleared by case 1 of recover()", log is None)
        assert_consistent(hosts, secrets, "before-finish")

        # Case 3: crash + prepared temp file lost -> unreplayable branch
        c3 = crash_case(root, seed_config, "before-rename", dylib, tag="lost-tmp")
        log_before = json.loads((c3 / ".commit.json").read_text())
        tmp = c3 / log_before["tmp_name"]
        before_hosts = (c3 / "hosts.json").read_bytes()
        print(f"\n$ rm {tmp.name}   # simulate a prepared temp file lost with the crash")
        tmp.unlink()
        hosts, secrets, log, p = restart(c3, "lost-tmp", expect_success=False, extra_name="must-not-land")
        check("lost-tmp: stderr explains the refusal",
              b"cannot be replayed" in p.stderr, p.stderr.decode(errors="replace").strip()[:160])
        check("lost-tmp: hosts.json unchanged", (c3 / "hosts.json").read_bytes() == before_hosts)
        check("lost-tmp: log kept for inspection", log is not None)
        check("lost-tmp: the refused host did not land",
              not any(h.get("name") == "must-not-land" for h in hosts.get("hosts", [])))

        print("\n=== summary ===")
        print(f"  cases=3 (before-rename, before-finish, lost-tmp)  failures={len(FAILURES)}")
        for f in FAILURES:
            print(f"  FAILED: {f}")
        return 1 if FAILURES else 0
    finally:
        # Keep the isolated tree for optional manual inspection, but report it.
        print(f"\n  isolated tree retained at {root}")


if __name__ == "__main__":
    sys.exit(main())
