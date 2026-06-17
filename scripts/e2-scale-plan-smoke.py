#!/usr/bin/env python3
"""Local scale smoke for 100-host planning without opening SSH connections."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


HOST_COUNT = int(os.environ.get("AGENT2SSH_SCALE_HOSTS", "100"))


def resolve_cli_bin() -> str:
    configured = os.environ.get("AGENT2SSH_BIN")
    if configured:
        return configured

    repo_root = Path(__file__).resolve().parents[1]
    local_debug = repo_root / "src-tauri" / "target" / "debug" / "agent2ssh"
    if local_debug.exists():
        return str(local_debug)

    found = shutil.which("agent2ssh")
    if found:
        return found

    raise SystemExit("agent2ssh not found. Set AGENT2SSH_BIN or build/install agent2ssh.")


def run(cli_bin: str, config_dir: str, args: list[str]) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["AGENT2SSH_CONFIG_DIR"] = config_dir
    return subprocess.run(
        [cli_bin, *args],
        text=True,
        capture_output=True,
        env=env,
        timeout=30,
        check=False,
    )


def main() -> int:
    cli_bin = resolve_cli_bin()
    with tempfile.TemporaryDirectory(prefix="agent2ssh-scale-") as config_dir:
        for i in range(1, HOST_COUNT + 1):
            name = f"scale-{i:03d}"
            proc = run(
                cli_bin,
                config_dir,
                [
                    "host",
                    "add",
                    name,
                    "--host",
                    f"192.0.2.{(i % 200) + 1}",
                    "--user",
                    "scale",
                    "--tags",
                    "scale",
                    "--env",
                    "test",
                    "--role",
                    "synthetic",
                    "--owner",
                    "e2",
                    "--json",
                ],
            )
            if proc.returncode != 0:
                raise RuntimeError(f"host add failed for {name}: {proc.stderr}")

        listed = run(cli_bin, config_dir, ["host", "list", "--tag", "scale", "--json"])
        if listed.returncode != 0:
            raise RuntimeError(f"host list failed: {listed.stderr}")
        hosts = json.loads(listed.stdout)
        if len(hosts) != HOST_COUNT:
            raise RuntimeError(f"expected {HOST_COUNT} hosts, got {len(hosts)}")

        host_names = [host["name"] for host in hosts]
        plan = run(
            cli_bin,
            config_dir,
            [
                "exec-multi",
                *host_names,
                "--command",
                "hostname",
                "--plan",
                "--json",
                "--timeout-secs",
                "5",
            ],
        )
        if plan.returncode != 0:
            raise RuntimeError(f"exec-multi plan failed: {plan.stderr}")
        plan_json = json.loads(plan.stdout)
        targets = plan_json.get("targets", [])
        if len(targets) != HOST_COUNT:
            raise RuntimeError(f"expected {HOST_COUNT} plan targets, got {len(targets)}")
        if plan_json.get("overall_risk") != "low":
            raise RuntimeError(f"expected low risk plan, got {plan_json.get('overall_risk')!r}")

        print(
            json.dumps(
                {
                    "cli_bin": cli_bin,
                    "hosts": len(hosts),
                    "plan_targets": len(targets),
                    "overall_risk": plan_json.get("overall_risk"),
                    "requires_approval": plan_json.get("requires_approval"),
                },
                indent=2,
            )
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
