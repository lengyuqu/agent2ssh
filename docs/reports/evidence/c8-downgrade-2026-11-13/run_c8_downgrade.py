#!/usr/bin/env python3
"""CNB issue #26, crash-matrix item C8: what an older build does to a tag.

Phase 2 A/B turned the on-disk credential reference from the bare marker
`$agent2ssh-secret$` into `$agent2ssh-secret$:<tag>`. Before that change the
marker test was an exact string comparison, so a build from `48ac012` (the last
commit before phase A/B) cannot recognise the tagged form. The design note
predicted that such a build treats the tagged reference as a **plaintext
password** and seals it again (F5); this script measures it with two real
binaries instead of inferring it from source.

The measurement also shows the damage needs no write command: the older CLI
runs `migrate_plaintext_secrets()` at startup for every command except
`completions`, `webdav decrypt` and `events`, so a single `host list` with an
unlocked store is enough. With the store locked the same command changes
nothing.

Every run uses throwaway config directories, a throwaway master password and
synthetic passwords. `~/.agent2ssh` is never touched.

Build the older CLI before running this:

    git worktree add /tmp/a2s_old 48ac012
    cd /tmp/a2s_old/src-tauri
    CARGO_TARGET_DIR=/tmp/a2s_old_target cargo build --no-default-features --bin agent2ssh
    python3 run_c8_downgrade.py --old-binary /tmp/a2s_old_target/debug/agent2ssh \
        --old-commit 48ac012 > raw-c8.txt 2>&1

Cases:
  baseline  the current build writes a tagged reference
  locked    the older build lists hosts with a locked store: nothing changes
  unlocked  the same listing with an unlocked store: the tag is destroyed
  reopen    the current build reads that pair: loud refusal, no silent use
  repeat    every further save by the older build seals the marker again
  repair    config-repair --restore-pair puts both files back byte-for-byte
"""
import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
DEFAULT_NEW = REPO / "src-tauri/target/debug/agent2ssh"
MASTER = "c8-synthetic-master"
PORTS = {"probe-a": 2222, "probe-b": 2223, "probe-c": 2224, "probe-d": 2225}

FAILURES = []


def run(cli, config_dir, args, stdin=None, unlock=True):
    env = dict(os.environ)
    env["AGENT2SSH_CONFIG_DIR"] = str(config_dir)
    if unlock:
        env["AGENT2SSH_MASTER_PASSWORD"] = MASTER
    else:
        env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    env.pop("AGENT2SSH_DAEMON_ADDR", None)
    return subprocess.run(
        [str(cli), *args],
        input=stdin,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )


def version(cli):
    return subprocess.run([str(cli), "--version"], capture_output=True, text=True).stdout.strip()


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def state(config_dir):
    """The parts of both files this issue is about."""
    hosts = json.loads((Path(config_dir) / "hosts.json").read_text())
    store = json.loads((Path(config_dir) / "secrets.enc").read_text())
    return {
        "hosts_generation": hosts.get("secrets_generation"),
        "refs": {h["name"]: h.get("password") for h in hosts.get("hosts", [])},
        "store_version": store.get("version"),
        "store_generation": store.get("generation"),
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
    print(f"  {label}: hosts_generation={s['hosts_generation']} "
          f"store_version={s['store_version']} store_generation={s['store_generation']}")
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


def add_host(cli, config_dir, name, password):
    return run(
        cli,
        config_dir,
        ["host", "add", name, "--host", "127.0.0.1", "--user", "probe",
         "--port", str(PORTS[name]), "--password-stdin"],
        stdin=password + "\n",
    )


def fresh(name):
    return Path(tempfile.mkdtemp(prefix=f"a2s-c8-{name}-"))


def baseline(new, config_dir):
    init = run(new, config_dir, ["secrets", "set-password", "--password", MASTER])
    if init.returncode != 0:
        print(init.stderr)
        raise SystemExit("could not initialise the credential store")
    added = add_host(new, config_dir, "probe-a", "c8-baseline-password")
    print(f"  host add probe-a: exit={added.returncode} stderr={added.stderr.strip()!r}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--old-binary", required=True)
    parser.add_argument("--new-binary", default=str(DEFAULT_NEW))
    parser.add_argument("--old-commit", default="unknown")
    args = parser.parse_args()

    old = Path(args.old_binary).resolve()
    new = Path(args.new_binary).resolve()
    for binary in (old, new):
        if not binary.is_file():
            raise SystemExit(f"missing binary: {binary}")

    print(f"date: {datetime.now(timezone.utc).isoformat(timespec='seconds')}")
    print(f"platform: {platform.platform()} ({platform.machine()}) uid={os.getuid()}")
    print(f"older build: {old}")
    print(f"  sha256 {digest(old)}  {version(old)}  commit {args.old_commit}")
    print(f"current build: {new}")
    print(f"  sha256 {digest(new)}  {version(new)}")

    work = fresh("main")
    repair_dir = fresh("repair")
    try:
        section("1. baseline: the current build writes a tagged reference")
        baseline(new, work)
        before = show("state", work)
        tag = before["refs"]["probe-a"].split(":", 1)[1]
        literal = f"$agent2ssh-secret$:{tag}"
        check(before["refs"]["probe-a"] == literal, "probe-a reference carries a content tag")
        check(before["entries"]["host:probe-a"]["tag"] == tag,
              "the store entry carries the same tag")
        check(before["store_version"] == 2, "store is still the v2 envelope")

        section("2. locked store: the older build only lists hosts")
        hosts_before = digest(work / "hosts.json")
        secrets_before = digest(work / "secrets.enc")
        locked = run(old, work, ["host", "list"], unlock=False)
        print(f"  host list (locked): exit={locked.returncode} stderr={locked.stderr.strip()!r}")
        print(f"  {locked.stdout.strip()}")
        check(digest(work / "hosts.json") == hosts_before
              and digest(work / "secrets.enc") == secrets_before,
              "a locked listing changed neither file")

        section("3. unlocked store: the same listing already destroys the tag")
        unlocked = run(old, work, ["host", "list"])
        print(f"  host list (unlocked): exit={unlocked.returncode}"
              f" stderr={unlocked.stderr.strip()!r}")
        print(f"  {unlocked.stdout.strip()}")
        after_list = show("state", work)
        check(after_list["refs"]["probe-a"] == "$agent2ssh-secret$",
              "probe-a's tagged reference was replaced by the bare marker")
        check(after_list["entries"]["host:probe-a"]["tag"] is None,
              "the store entry lost its tag (the older build writes the v2 shape)")
        check(after_list["hosts_generation"] == before["hosts_generation"] + 1,
              "the startup migration bumped the generation")
        check((work / "hosts.json.bak").exists(),
              "the migration also left a hosts.json.bak of the previous file")
        # base64 of (plaintext + 16-byte AEAD tag); the plaintext here is the
        # 41-character reference text, which is why the ciphertext grows to 76.
        check(after_list["entries"]["host:probe-a"]["ciphertext_len"] == 76,
              "host:probe-a now holds a 41-character plaintext (the reference text)")
        print(f"  the sealed text is {literal!r} ({len(literal)} chars)")

        section("4. reopen: the current build reads the downgraded pair")
        reopened = add_host(new, work, "probe-c", "c8-third-password")
        print(f"  host add probe-c: exit={reopened.returncode}"
              f" stderr={reopened.stderr.strip()!r}")
        after_new = show("state", work)
        check(after_new["refs"]["probe-a"] == literal,
              "probe-a's reference came back as the exact tagged text it was sealed as")
        check(after_new["entries"]["host:probe-a"]["tag"] == "",
              "the entry has no tag, so the reference cannot resolve")
        warned = run(new, work, ["host", "list"])
        print(f"  host list stderr: {warned.stderr.strip()!r}")
        check("is NOT used" in warned.stderr,
              "the current build refuses the credential instead of using it")

        section("5. repeat: every further save by the older build seals it again")
        again = add_host(old, work, "probe-d", "c8-fourth-password")
        print(f"  host add probe-d: exit={again.returncode} stderr={again.stderr.strip()!r}")
        after_repeat = show("state", work)
        check(after_repeat["refs"]["probe-a"] == "$agent2ssh-secret$",
              "probe-a is back to the bare marker")
        check(after_repeat["entries"]["host:probe-a"]["ciphertext_len"] == 76,
              "host:probe-a still holds reference text, not the original password")
        check(after_repeat["entries"]["host:probe-c"]["ciphertext_len"] == 76,
              "host:probe-c was sealed the same way")

        section("6. repair: the paired snapshot restores both files")
        baseline(new, repair_dir)
        clean_hosts = digest(repair_dir / "hosts.json")
        clean_secrets = digest(repair_dir / "secrets.enc")
        damaged = add_host(old, repair_dir, "probe-b", "c8-second-password")
        print(f"  older build save: exit={damaged.returncode}"
              f" stderr={damaged.stderr.strip()!r}")
        show("damaged", repair_dir)
        preview = run(new, repair_dir, ["config-repair", "--restore-pair", "0"])
        print(f"  restore preview: exit={preview.returncode}")
        for line in preview.stdout.strip().splitlines():
            print(f"    {line}")
        applied = run(new, repair_dir, ["config-repair", "--restore-pair", "0", "--confirm"])
        print(f"  restore applied: exit={applied.returncode}")
        for line in applied.stdout.strip().splitlines():
            print(f"    {line}")
        check(digest(repair_dir / "hosts.json") == clean_hosts,
              "hosts.json is byte-identical to the pre-downgrade file")
        check(digest(repair_dir / "secrets.enc") == clean_secrets,
              "secrets.enc is byte-identical to the pre-downgrade file")
        after_repair = show("repaired", repair_dir)
        check(after_repair["entries"]["host:probe-a"]["tag"] is not None,
              "the restored entry carries its tag again")
        followed = add_host(new, repair_dir, "probe-c", "c8-sixth-password")
        print(f"  later save: exit={followed.returncode} stderr={followed.stderr.strip()!r}")
        check(followed.stderr.strip() == "", "the repaired pair loads without a tag warning")
        check(state(repair_dir)["entries"]["host:probe-a"]["tag"] is not None,
              "the credential still resolves after the later save")
    finally:
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(repair_dir, ignore_errors=True)

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
