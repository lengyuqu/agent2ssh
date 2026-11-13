#!/usr/bin/env python3
"""CNB issue #38: real SIGKILL injection into the `secrets retag` window.

`agent2ssh secrets retag` rewrites `secrets.enc` (fresh content tags on every
entry that lacks one) and then `hosts.json` (bare `$agent2ssh-secret$`
references upgraded to `$agent2ssh-secret$:<tag>`), under the phase-0/1
commit protocol. This script freezes the real CLI at both renames with the
DYLD-interpose hook from `tests/fixtures/crash_hook.c`, kills it from outside
with SIGKILL, and proves that a restarted retag completes the pair
idempotently with the plaintext untouched.

Stages exercised (A2S_HOOK_STAGE):
  before-restore-secrets  frozen before `secrets.enc` is replaced: nothing
                          changed, no `.commit.json`, rerun retags from scratch
  before-rename           frozen after `secrets.enc` was retagged and
                          `.commit.json` was written, before the `hosts.json`
                          rename: rerun replays/recovers and completes

Every run uses throwaway config directories, a throwaway master password and
synthetic passwords. `~/.agent2ssh` is never touched.

Build the CLI first, then run:

    cd src-tauri && cargo build --no-default-features --bin agent2ssh
    python3 ../docs/reports/evidence/issue38-retag-2026-11-13/run_retag_crash.py \
        > raw-retag-crash.txt 2>&1
"""
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
CLI = REPO / "src-tauri/target/debug/agent2ssh"
HOOK_SRC = REPO / "src-tauri/tests/fixtures/crash_hook.c"

MASTER = "retag-crash-synthetic-master"
SECRET = "retag-crash-synthetic-secret-marker"
HOST = "retag-crash-host"
ACCOUNT = "host:retag-crash-host"
BARE = "$agent2ssh-secret$"

FAILURES = []


def check(label, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {label}" + (f" — {detail}" if detail else ""))
    if not ok:
        FAILURES.append(label)


def cli_env(config_dir, unlock=True, extra_env=None):
    env = dict(os.environ)
    env["AGENT2SSH_CONFIG_DIR"] = str(config_dir)
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    env.pop("DYLD_INSERT_LIBRARIES", None)
    env.pop("A2S_HOOK_STAGE", None)
    env.pop("A2S_HOOK_DIR", None)
    if unlock:
        env["AGENT2SSH_MASTER_PASSWORD"] = MASTER
    if extra_env:
        env.update(extra_env)
    return env


def run(config_dir, args, unlock=True, extra_env=None):
    return subprocess.run(
        [str(CLI), *args],
        env=cli_env(config_dir, unlock, extra_env),
        capture_output=True,
        text=True,
        timeout=120,
    )


def read_json(path):
    return json.loads(Path(path).read_text())


def make_legacy(config_dir):
    """Rewrite the seeded pair into the pre-phase-2 shape: bare marker + empty tags."""
    hosts = read_json(config_dir / "hosts.json")
    for host in hosts["hosts"]:
        if host.get("name") == HOST:
            host["password"] = BARE
    (config_dir / "hosts.json").write_text(json.dumps(hosts, indent=2))
    store = read_json(config_dir / "secrets.enc")
    for entry in store["entries"].values():
        entry["tag"] = ""
    (config_dir / "secrets.enc").write_text(json.dumps(store, indent=2))


def seed(config_dir):
    result = subprocess.run(
        [str(CLI), "secrets", "set-password"],
        env=cli_env(config_dir, unlock=False),
        input=MASTER + "\n",
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert result.returncode == 0, result.stderr
    result = subprocess.run(
        [str(CLI), "host", "add", HOST, "--host", "127.0.0.1", "--password-stdin"],
        env=cli_env(config_dir),
        input=SECRET + "\n",
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert result.returncode == 0, result.stderr


def crash_at(config_dir, dylib, hook_dir, stage, args):
    """Freeze the real CLI at `stage`, then SIGKILL it from this process."""
    hook_dir.mkdir(parents=True, exist_ok=True)
    env = cli_env(config_dir, extra_env={
        "DYLD_INSERT_LIBRARIES": str(dylib),
        "A2S_HOOK_STAGE": stage,
        "A2S_HOOK_DIR": str(hook_dir),
    })
    proc = subprocess.Popen(
        [str(CLI), *args], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    fired = hook_dir / f"{stage}.fired"
    deadline = time.time() + 20
    while not fired.exists():
        if time.time() > deadline:
            proc.kill()
            raise AssertionError(f"hook stage {stage} never fired")
        time.sleep(0.01)
    proc.kill()  # external SIGKILL
    proc.wait(timeout=30)
    check(f"{stage}: process died from SIGKILL", proc.returncode == -9, f"rc={proc.returncode}")


def verify_complete(config_dir, label):
    hosts = read_json(config_dir / "hosts.json")
    store = read_json(config_dir / "secrets.enc")
    reference = None
    for host in hosts["hosts"]:
        if host.get("name") == HOST:
            reference = host["password"]
    ok_tagged = reference and reference.startswith(BARE + ":")
    tag = reference.split(":", 1)[1] if ok_tagged else None
    entry_tag = store["entries"].get(ACCOUNT, {}).get("tag")
    check(f"{label}: reference is tagged", bool(ok_tagged), str(reference))
    check(f"{label}: tag matches store entry", tag == entry_tag, f"{tag} vs {entry_tag}")
    check(
        f"{label}: generations aligned",
        hosts.get("secrets_generation") == store.get("generation"),
        f"hosts={hosts.get('secrets_generation')} store={store.get('generation')}",
    )
    check(f"{label}: no .commit.json left", not (config_dir / ".commit.json").exists())
    # The plaintext behind the reference is unchanged: decrypt via a fresh CLI.
    result = run(config_dir, ["secrets", "retag", "--confirm"])
    check(f"{label}: rerun is a no-op", "Nothing to retag" in result.stdout, result.stdout.strip())


def build_dylib(workdir):
    src = workdir / "crash_hook.c"
    dylib = workdir / "liba2s_crashhook.dylib"
    src.write_text(HOOK_SRC.read_text())
    subprocess.run(
        ["clang", "-dynamiclib", "-o", str(dylib), str(src)],
        check=True,
        capture_output=True,
    )
    return dylib


def main():
    if not CLI.exists():
        sys.exit(f"CLI not built: {CLI}")
    print(f"== issue #38 retag crash injection ==")
    print(f"cli: {CLI}")
    print(f"platform: {sys.platform}")

    with tempfile.TemporaryDirectory(prefix="a2s-retag-crash-") as tmp:
        workdir = Path(tmp)
        dylib = build_dylib(workdir)

        # Stage 1: freeze before secrets.enc is replaced.
        print("\n-- stage before-restore-secrets --")
        config = workdir / "config-secrets"
        seed(config)
        make_legacy(config)
        crash_at(
            config, dylib, workdir / "hook-secrets", "before-restore-secrets",
            ["secrets", "retag", "--confirm"],
        )
        secrets_now = (config / "secrets.enc").read_bytes()
        hosts_now = (config / "hosts.json").read_bytes()
        check(
            "before-restore-secrets: hosts.json untouched",
            hosts_now == (config / "hosts.json").read_bytes() and BARE in hosts_now.decode(),
        )
        store = json.loads(secrets_now)
        check(
            "before-restore-secrets: tags still empty",
            all(e["tag"] == "" for e in store["entries"].values()),
        )
        check(
            "before-restore-secrets: no .commit.json",
            not (config / ".commit.json").exists(),
        )
        rerun = run(config, ["secrets", "retag", "--confirm"])
        check("before-restore-secrets: rerun succeeds", rerun.returncode == 0, rerun.stderr)
        verify_complete(config, "before-restore-secrets/restart")

        # Stage 2: freeze after secrets were retagged + .commit.json written,
        # before the hosts.json rename.
        print("\n-- stage before-rename --")
        config = workdir / "config-hosts"
        seed(config)
        make_legacy(config)
        crash_at(
            config, dylib, workdir / "hook-hosts", "before-rename",
            ["secrets", "retag", "--confirm"],
        )
        check("before-rename: .commit.json present", (config / ".commit.json").exists())
        check("before-rename: hosts.json still bare", BARE in (config / "hosts.json").read_text())
        store = json.loads((config / "secrets.enc").read_text())
        check(
            "before-rename: secrets already retagged",
            store["entries"][ACCOUNT]["tag"] != "",
        )
        rerun = run(config, ["secrets", "retag", "--confirm"])
        check("before-rename: rerun succeeds", rerun.returncode == 0, rerun.stderr)
        verify_complete(config, "before-rename/restart")

    print("\n== summary ==")
    if FAILURES:
        print(f"{len(FAILURES)} FAILED: {FAILURES}")
        sys.exit(1)
    print("all checks passed")


if __name__ == "__main__":
    main()
