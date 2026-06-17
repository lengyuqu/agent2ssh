#!/usr/bin/env python3
"""Protocol-level MCP smoke for common agent client source labels.

This verifies that the same agent2ssh-mcp stdio server can initialize, list
tools, and execute a safe read-only tool call when launched with source labels
used by different agent clients. It intentionally does not automate client UIs.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


CLIENT_SOURCES = ["codex", "opencode", "cursor", "claude-code"]


def resolve_mcp_bin() -> str:
    configured = os.environ.get("AGENT2SSH_MCP_BIN")
    if configured:
        return configured

    repo_root = Path(__file__).resolve().parents[1]
    local_debug = repo_root / "src-tauri" / "target" / "debug" / "agent2ssh-mcp"
    if local_debug.exists():
        return str(local_debug)

    found = shutil.which("agent2ssh-mcp")
    if found:
        return found

    raise SystemExit(
        "agent2ssh-mcp not found. Set AGENT2SSH_MCP_BIN or build/install agent2ssh-mcp."
    )


def call_mcp(mcp_bin: str, source: str) -> list[dict]:
    env = os.environ.copy()
    env["AGENT2SSH_SOURCE"] = source
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
        {
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "ssh_risk_check",
                "arguments": {"command": "rm -rf /"},
            },
        },
    ]
    stdin = "\n".join(json.dumps(req) for req in requests) + "\n"
    proc = subprocess.run(
        [mcp_bin],
        input=stdin,
        text=True,
        capture_output=True,
        env=env,
        timeout=20,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"{source}: mcp process exited {proc.returncode}\nSTDERR:\n{proc.stderr}"
        )
    responses = [json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
    if len(responses) != 3:
        raise RuntimeError(f"{source}: expected 3 JSON-RPC responses, got {len(responses)}")
    return responses


def verify_source(mcp_bin: str, source: str) -> dict:
    responses = call_mcp(mcp_bin, source)
    init, tools, risk = responses

    server_name = init.get("result", {}).get("serverInfo", {}).get("name")
    if server_name != "agent2ssh-mcp":
        raise RuntimeError(f"{source}: unexpected server name {server_name!r}")

    tool_list = tools.get("result", {}).get("tools", [])
    tool_names = {tool.get("name") for tool in tool_list}
    required = {"ssh_list_hosts", "ssh_exec", "ssh_exec_multi", "ssh_risk_check"}
    missing = sorted(required - tool_names)
    if missing:
        raise RuntimeError(f"{source}: missing required tools {missing}")

    risk_level = (
        risk.get("result", {})
        .get("structuredContent", {})
        .get("risk_level")
    )
    if risk_level != "blocked":
        raise RuntimeError(f"{source}: expected blocked risk, got {risk_level!r}")

    return {
        "source": source,
        "server": server_name,
        "tools": len(tool_list),
        "risk_check": risk_level,
    }


def main() -> int:
    mcp_bin = resolve_mcp_bin()
    results = [verify_source(mcp_bin, source) for source in CLIENT_SOURCES]
    print(json.dumps({"mcp_bin": mcp_bin, "results": results}, indent=2))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(1)
