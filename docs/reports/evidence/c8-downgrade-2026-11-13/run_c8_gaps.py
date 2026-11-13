#!/usr/bin/env python3
"""CNB issue #26, C8 follow-up: the two paths the first measurement left open.

`run_c8_downgrade.py` drove the older **CLI** and found that one unlocked
`host list` is enough to destroy a tagged reference. Two paths stayed
unmeasured:

  * interactive unlock — the older CLI never prompts for a master password for
    ordinary commands, but the desktop settings UI unlocks the store through the
    Tauri command `secrets_unlock`, which then runs the same migration;
  * the daemon binary — `agent2ssh-daemon` migrates at startup, before it binds
    its listener.

This script measures both with builds of the same pre-phase-2 commit, adds a
locked-store negative case for each, and contrasts the current daemon. The
desktop GUI itself is not launched (it needs a window server): `desktop-unlock-
probe.rs` next to this file is compiled against the same commit with the
`tauri` feature and calls the real `tauri_commands::secrets_unlock` body — the
exact function the settings UI invokes.

All runs use throwaway config directories, a throwaway master password and
synthetic passwords; `~/.agent2ssh` is never touched.

Build the older artifacts first:

    git worktree add /tmp/a2s_old 48ac012
    cd /tmp/a2s_old/src-tauri
    cp <this directory>/desktop-unlock-probe.rs src/bin/c8_desktop_unlock_probe.rs
    CARGO_TARGET_DIR=/tmp/a2s_old_target cargo build --no-default-features \
        --features daemon --bin agent2ssh --bin agent2ssh-daemon
    CARGO_TARGET_DIR=/tmp/a2s_old_target cargo build --features tauri \
        --bin c8_desktop_unlock_probe
    rm src/bin/c8_desktop_unlock_probe.rs   # the older tree stays untouched

    python3 run_c8_gaps.py \
        --old-binary /tmp/a2s_old_target/debug/agent2ssh \
        --old-daemon /tmp/a2s_old_target/debug/agent2ssh-daemon \
        --desktop-probe /tmp/a2s_old_target/debug/c8_desktop_unlock_probe \
        --old-commit 48ac012 > raw-c8-gaps.txt 2>&1
"""
import argparse
import hashlib
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
DEFAULT_NEW = REPO / "src-tauri/target/debug/agent2ssh"
DEFAULT_NEW_DAEMON = REPO / "src-tauri/target/debug/agent2ssh-daemon"
MASTER = "c8-synthetic-master"

FAILURES = []


def base_env(config_dir, unlock):
    env = dict(os.environ)
    env["AGENT2SSH_CONFIG_DIR"] = str(config_dir)
    if unlock:
        env["AGENT2SSH_MASTER_PASSWORD"] = MASTER
    else:
        env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    env.pop("AGENT2SSH_DAEMON_ADDR", None)
    return env


def run(cli, config_dir, args, stdin=None, unlock=True, timeout=120):
    return subprocess.run(
        [str(cli), *args],
        input=stdin,
        env=base_env(config_dir, unlock),
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def version(cli, config_dir):
    """Version banner of a CLI binary, run against a throwaway config dir.

    Two hazards this guards against, both hit during development of this
    script: a binary that ignores argv starts a real service instead of
    printing a version (so this is only called for CLIs), and any of them run
    without AGENT2SSH_CONFIG_DIR would reach the user's real ~/.agent2ssh.
    """
    try:
        return subprocess.run(
            [str(cli), "--version"],
            env=base_env(config_dir, unlock=False),
            capture_output=True,
            text=True,
            timeout=15,
        ).stdout.strip()
    except subprocess.TimeoutExpired:
        return "unknown (timed out)"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def pair_digest(config_dir):
    return digest(Path(config_dir) / "hosts.json"), digest(Path(config_dir) / "secrets.enc")


def state(config_dir):
    hosts = json.loads((Path(config_dir) / "hosts.json").read_text())
    store = json.loads((Path(config_dir) / "secrets.enc").read_text())
    return {
        "hosts_generation": hosts.get("secrets_generation"),
        "names": [h["name"] for h in hosts.get("hosts", [])],
        "refs": {h["name"]: h.get("password") for h in hosts.get("hosts", [])},
        "store_version": store.get("version"),
        "entries": {
            account: {
                "tag": entry.get("tag"),
                "ciphertext_len": len(entry.get("ciphertext", "")),
            }
            for account, entry in sorted(store.get("entries", {}).items())
        },
    }


def show(label, config_dir):
    s = state(config_dir)
    print(f"  {label}: hosts_generation={s['hosts_generation']} store_version={s['store_version']}")
    for name, ref in s["refs"].items():
        print(f"    ref {name:8s} = {ref!r}")
    for account, entry in s["entries"].items():
        print(f"    entry {account:26s} tag={entry['tag']!r} ciphertext_len={entry['ciphertext_len']}")
    return s


def check(condition, message):
    print(("  PASS " if condition else "  FAIL ") + message)
    if not condition:
        FAILURES.append(message)


def section(title):
    print()
    print("=" * 78)
    print(title)
    print("=" * 78)


def fresh(name):
    return Path(tempfile.mkdtemp(prefix=f"a2s-c8g-{name}-"))


def baseline(new, config_dir):
    init = run(new, config_dir, ["secrets", "set-password", "--password", MASTER])
    if init.returncode != 0:
        print(init.stderr)
        raise SystemExit("could not initialise the credential store")
    added = run(
        new,
        config_dir,
        ["host", "add", "probe-a", "--host", "127.0.0.1", "--user", "probe",
         "--port", "2222", "--password-stdin"],
        stdin="c8-baseline-password\n",
    )
    if added.returncode != 0:
        print(added.stderr)
        raise SystemExit("could not add the baseline host")


def free_port():
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    return port


def start_daemon(daemon, config_dir, unlock):
    """Start the daemon and wait for /health; returns (process, health body).

    Output goes to a file, not a pipe: an unread pipe fills up and blocks the
    daemon mid-write.
    """
    port = free_port()
    env = base_env(config_dir, unlock)
    env["AGENT2SSH_DAEMON_ADDR"] = f"127.0.0.1:{port}"
    log_path = Path(config_dir) / "daemon.stdout.log"
    with log_path.open("w") as log:
        process = subprocess.Popen([str(daemon)], env=env, stdout=log, stderr=subprocess.STDOUT)
    health = ""
    for _ in range(75):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=0.5) as response:
                health = response.read().decode("utf-8", "replace")
            break
        except (urllib.error.URLError, OSError):
            if process.poll() is not None:
                break
            time.sleep(0.2)
    return process, health


def stop_daemon(process):
    process.terminate()
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)
    return process.returncode


def daemon_case(new, daemon, config_dir, unlock, label):
    """Run one daemon-startup case; returns the state before and after."""
    baseline(new, config_dir)
    before = show("before", config_dir)
    process, health = start_daemon(daemon, config_dir, unlock)
    try:
        print(f"  {label} /health: {health[:70]!r}")
        after = show("after", config_dir)
        check(bool(health), f"{label}: the daemon answered /health")
    finally:
        exit_code = stop_daemon(process)
        print(f"  {label} daemon exit: {exit_code}")
        log_path = Path(config_dir) / "daemon.stdout.log"
        if log_path.exists():
            tail = log_path.read_text(errors="replace").strip().splitlines()[-2:]
            for line in tail:
                print(f"    {label} log: {line[:150]}")
    return before, after


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(line_buffering=True)
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-binary", required=True)
    parser.add_argument("--old-daemon", required=True)
    parser.add_argument("--desktop-probe", required=True)
    parser.add_argument("--new-binary", default=str(DEFAULT_NEW))
    parser.add_argument("--new-daemon", default=str(DEFAULT_NEW_DAEMON))
    parser.add_argument("--old-commit", default="unknown")
    args = parser.parse_args()

    binaries = {
        "older cli": Path(args.old_binary).resolve(),
        "older daemon": Path(args.old_daemon).resolve(),
        "older desktop-unlock probe": Path(args.desktop_probe).resolve(),
        "current cli": Path(args.new_binary).resolve(),
        "current daemon": Path(args.new_daemon).resolve(),
    }
    for label, binary in binaries.items():
        if not binary.is_file():
            raise SystemExit(f"missing {label}: {binary}")

    directories = []
    banner_dir = fresh("banner")
    directories.append(banner_dir)
    print(f"date: {datetime.now(timezone.utc).isoformat(timespec='seconds')}")
    print(f"platform: {platform.platform()} ({platform.machine()}) uid={os.getuid()}")
    for label, binary in binaries.items():
        extra = f"  commit {args.old_commit}" if label.startswith("older") else ""
        # Only real CLIs understand --version. The daemon ignores argv and the
        # desktop probe treats argv[1] as the master password, so asking either
        # of them for a version starts work instead of printing one.
        banner = f"  {version(binary, banner_dir)}" if label.endswith("cli") else ""
        print(f"{label}: {binary}")
        print(f"  sha256 {digest(binary)}{banner}{extra}")

    try:
        section("1. older CLI, locked store, master password on stdin")
        work = fresh("cli-stdin")
        directories.append(work)
        baseline(args.new_binary, work)
        before = pair_digest(work)
        locked = run(
            args.old_binary,
            work,
            ["host", "add", "probe-b", "--host", "127.0.0.1", "--user", "probe",
             "--port", "2223", "--password-stdin"],
            stdin=MASTER + "\n",
            unlock=False,
        )
        last_line = locked.stderr.strip().splitlines()[-1] if locked.stderr.strip() else ""
        print(f"  host add: exit={locked.returncode} last stderr line={last_line!r}")
        after = show("state", work)
        check(locked.returncode != 0, "the command failed instead of unlocking from stdin")
        check("locked" in locked.stderr, "the error says the store is locked")
        check(pair_digest(work) == before, "neither file changed")
        check(after["names"] == ["probe-a"], "no host was added")
        check(after["refs"]["probe-a"].startswith("$agent2ssh-secret$:"),
              "the tagged reference is intact")

        section("2. older desktop unlock command (the Tauri command body)")
        work = fresh("desktop")
        directories.append(work)
        baseline(args.new_binary, work)
        before = show("before", work)
        tag = before["refs"]["probe-a"].split(":", 1)[1]
        unlocked = subprocess.run(
            [str(binaries["older desktop-unlock probe"]), MASTER],
            env=base_env(work, unlock=False),
            capture_output=True,
            text=True,
            timeout=120,
        )
        print(f"  secrets_unlock: exit={unlocked.returncode} stdout={unlocked.stdout.strip()!r}"
              f" stderr={unlocked.stderr.strip()!r}")
        after = show("after", work)
        check(unlocked.returncode == 0, "the unlock command reported success")
        check(after["refs"]["probe-a"] == "$agent2ssh-secret$",
              "the tagged reference became the bare marker")
        check(after["entries"]["host:probe-a"]["tag"] is None, "the store entry lost its tag")
        check(after["hosts_generation"] == before["hosts_generation"] + 1,
              "the migration bumped the generation")
        check(after["entries"]["host:probe-a"]["ciphertext_len"] == 76,
              "the entry now holds the 41-character reference text")
        check((work / "hosts.json.bak").exists(), "a hosts.json.bak was left behind")
        print(f"  the sealed text is '$agent2ssh-secret$:{tag}'")

        section("3. older daemon, locked store")
        work = fresh("daemon-locked")
        directories.append(work)
        before, after = daemon_case(args.new_binary, args.old_daemon, work, unlock=False,
                                    label="older daemon (locked)")
        check(after["refs"]["probe-a"] == before["refs"]["probe-a"],
              "locked daemon: the tagged reference is intact")
        check(after["entries"]["host:probe-a"]["tag"] is not None,
              "locked daemon: the store entry keeps its tag")

        section("4. older daemon, master password in the environment")
        work = fresh("daemon-unlocked")
        directories.append(work)
        before, after = daemon_case(args.new_binary, args.old_daemon, work, unlock=True,
                                    label="older daemon (unlocked)")
        check(after["refs"]["probe-a"] == "$agent2ssh-secret$",
              "startup alone replaced the tagged reference with the bare marker")
        check(after["entries"]["host:probe-a"]["tag"] is None,
              "the store entry lost its tag before any client request")
        check(after["hosts_generation"] == before["hosts_generation"] + 1,
              "the startup migration bumped the generation")
        check(after["entries"]["host:probe-a"]["ciphertext_len"] == 76,
              "the entry now holds the 41-character reference text")
        check((work / "hosts.json.bak").exists(), "a hosts.json.bak was left behind")

        section("5. contrast: the current daemon on the same tagged config")
        work = fresh("daemon-current")
        directories.append(work)
        before, after = daemon_case(args.new_binary, args.new_daemon, work, unlock=True,
                                    label="current daemon")
        check(after["refs"]["probe-a"] == before["refs"]["probe-a"],
              "the current daemon leaves the tagged reference alone")
        check(after["entries"]["host:probe-a"]["tag"] == before["entries"]["host:probe-a"]["tag"],
              "the store entry keeps the same tag")
        followed = run(
            args.new_binary,
            work,
            ["host", "add", "probe-b", "--host", "127.0.0.1", "--user", "probe",
             "--port", "2223", "--password-stdin"],
            stdin="c8-second-password\n",
        )
        print(f"  later save: exit={followed.returncode} stderr={followed.stderr.strip()!r}")
        check(followed.stderr.strip() == "", "a later save emits no tag warning")
        check(state(work)["entries"]["host:probe-a"]["tag"] is not None,
              "the credential still carries its tag after that save")
    finally:
        for directory in directories:
            shutil.rmtree(directory, ignore_errors=True)

    section("summary")
    if FAILURES:
        print(f"  {len(FAILURES)} check(s) failed:")
        for failure in FAILURES:
            print(f"   - {failure}")
        return 1
    print("  every check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
