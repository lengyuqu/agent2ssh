#!/usr/bin/env python3
"""Issue #27 — the classic-conhost half of the no-echo question.

wexpect creates a real console (conhost) and reads its screen buffer, so what
this script sees is what a person looking at a cmd.exe window would see. That is
a different code path from ConPTY: ConPTY has its own VT renderer, while classic
conhost is the Win32 console host that `SetConsoleMode` was designed for.

Passwords are synthetic; each case owns a throwaway config dir.
"""

import json
import os
import subprocess
import sys
import tempfile
import time
import uuid

import wexpect

results = []


def cfg_dir(tag):
    path = os.path.join(os.environ.get("TEMP", "."), "a2s27-conhost-" + tag + "-" + uuid.uuid4().hex[:6])
    os.makedirs(path, exist_ok=True)
    return path


def unlock_check(exe, cfg, password):
    env = os.environ.copy()
    env["AGENT2SSH_CONFIG_DIR"] = cfg
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env["AGENT2SSH_MASTER_PASSWORD"] = password
    proc = subprocess.run([exe, "secrets", "status", "--json"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    try:
        payload = json.loads(proc.stdout.decode("utf-8", "replace"))
    except json.JSONDecodeError:
        return None
    return bool(payload.get("unlocked"))


def run_case(exe, tag, keys, visible, verify_password):
    """keys: bytes sent as keystrokes; visible: what a reader would see if
    echo were on; verify_password: the value the store must end up with."""
    cfg = cfg_dir(tag)
    saved = {key: os.environ.get(key) for key in
             ("AGENT2SSH_CONFIG_DIR", "AGENT2SSH_SECRETS_BACKEND", "AGENT2SSH_MASTER_PASSWORD")}
    os.environ["AGENT2SSH_CONFIG_DIR"] = cfg
    os.environ["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    os.environ.pop("AGENT2SSH_MASTER_PASSWORD", None)
    seen = ""
    row = {"case": tag, "backend": "conhost", "config_dir": cfg}
    try:
        child = wexpect.spawn(exe, ["secrets", "set-password"], timeout=20, dimensions=(30, 120))
        index = child.expect(["New master password:", wexpect.EOF, wexpect.TIMEOUT])
        row["prompt1"] = index == 0
        seen += child.before or ""
        if index == 0:
            child.send(keys)
            child.send("\r")
            index2 = child.expect(["Confirm master password:", wexpect.EOF, wexpect.TIMEOUT])
            row["prompt2"] = index2 == 0
            seen += child.before or ""
            if index2 == 0:
                child.send(verify_password)
                child.send("\r")
                index3 = child.expect(["Master password set", wexpect.EOF, wexpect.TIMEOUT])
                row["completed"] = index3 == 0
                seen += (child.before or "") + (child.after or "")
        row["screen_contains_password"] = visible in seen
        row["screen_contains_confirm_value"] = verify_password in seen
        row["screen"] = seen
        try:
            child.close(force=True)
        except Exception:
            pass
    except Exception as error:  # noqa: BLE001 - the report needs the failure text
        row["error"] = repr(error)
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    row["unlock_with_expected"] = unlock_check(exe, cfg, verify_password)
    results.append(row)
    print(json.dumps({k: row.get(k) for k in
                      ("case", "backend", "prompt1", "prompt2", "completed",
                       "screen_contains_password", "unlock_with_expected", "error")},
                     ensure_ascii=False))
    return row


def run_ctrl_c(exe, tag):
    """Ctrl+C while the echo is off: with ENABLE_PROCESSED_INPUT cleared the
    console must not raise CTRL_C_EVENT; the byte has to reach the CLI, which
    cancels the prompt instead of dying mid-initialisation."""
    cfg = cfg_dir(tag)
    saved = {key: os.environ.get(key) for key in
             ("AGENT2SSH_CONFIG_DIR", "AGENT2SSH_SECRETS_BACKEND", "AGENT2SSH_MASTER_PASSWORD")}
    os.environ["AGENT2SSH_CONFIG_DIR"] = cfg
    os.environ["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    os.environ.pop("AGENT2SSH_MASTER_PASSWORD", None)
    row = {"case": tag, "backend": "conhost", "config_dir": cfg}
    seen = ""
    try:
        child = wexpect.spawn(exe, ["secrets", "set-password"], timeout=20, dimensions=(30, 120))
        index = child.expect(["New master password:", wexpect.EOF, wexpect.TIMEOUT])
        row["prompt1"] = index == 0
        seen += child.before or ""
        if index == 0:
            child.send("\x03")
            index2 = child.expect(["cancelled", wexpect.EOF, wexpect.TIMEOUT])
            row["cancelled"] = index2 == 0
            seen += (child.before or "") + (child.after or "")
        row["created_store"] = os.path.exists(os.path.join(cfg, "secrets.enc"))
        row["screen"] = seen
        try:
            child.close(force=True)
        except Exception:
            pass
    except Exception as error:  # noqa: BLE001
        row["error"] = repr(error)
    finally:
        for key, value in saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    results.append(row)
    print(json.dumps({k: row.get(k) for k in
                      ("case", "backend", "prompt1", "cancelled", "created_store", "error")},
                     ensure_ascii=False))


def main():
    exe = os.path.abspath(sys.argv[1])
    outdir = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else "."

    secret = "SYNTHETIC_CONHOST_PASSWORD_7K"
    run_case(exe, "conhost-plain", secret, secret, secret)

    # Backspace: `abX` then DEL then `c` must store "abc", and a reader must see
    # only the characters as typed (with X erased), never a stored value echo.
    run_case(exe, "conhost-backspace", "abX\x7fc", "abXc", "abc")

    cjk = "合成口令-测试"
    run_case(exe, "conhost-cjk", cjk, cjk, cjk)

    run_ctrl_c(exe, "conhost-ctrl-c")

    with open(os.path.join(outdir, "raw-conhost-probe.json"), "w", encoding="utf-8") as handle:
        json.dump(results, handle, ensure_ascii=False, indent=2)
    print("wrote", os.path.join(outdir, "raw-conhost-probe.json"))


if __name__ == "__main__":
    main()
