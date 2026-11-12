#!/usr/bin/env python3
"""Issue #27 — is `secrets set-password` really no-echo inside a real terminal?

Backends: ConPTY (Windows Terminal, VS Code, anything on `CreatePseudoConsole`)
via pywinpty. Classic conhost is a separate console implementation; its screen
buffer is what a human sees, so it is worth its own backend once this one is
understood.

Every case runs against its own throwaway config dir; the password is checked
afterwards by asking a fresh CLI whether `AGENT2SSH_MASTER_PASSWORD` unlocks the
store it just created, so "the value arrived intact" is measured, not assumed.
"""

import json
import os
import subprocess
import sys
import tempfile
import time
import uuid

import winpty

results = []


def cfg_dir(tag):
    path = os.path.join(os.environ.get("TEMP", "."), "a2s27-pty-" + tag + "-" + uuid.uuid4().hex[:6])
    os.makedirs(path, exist_ok=True)
    return path


def base_env(cfg):
    env = os.environ.copy()
    env["AGENT2SSH_CONFIG_DIR"] = cfg
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    return env


def drain(pty, seconds, quiet_window=0.3):
    """Read until `seconds` of silence, capped at `seconds`."""
    buf = ""
    deadline = time.time() + seconds
    while time.time() < deadline:
        chunk = ""
        try:
            chunk = pty.read(blocking=False)
        except winpty.WinptyError:
            pass
        if chunk:
            buf += chunk
            deadline = time.time() + quiet_window
        else:
            time.sleep(0.03)
    return buf


class Reader:
    """Accumulates PTY output and searches forward from a cursor.

    A prompt can arrive in the same read as the previous one, so each search
    must continue from where the last successful match ended instead of
    rescanning (and instead of throwing the buffer away).
    """

    def __init__(self, pty):
        self.pty = pty
        self.buf = ""
        self.cursor = 0

    def expect(self, needle, timeout=20):
        deadline = time.time() + timeout
        while True:
            found = self.buf.find(needle, self.cursor)
            if found >= 0:
                self.cursor = found + len(needle)
                return True
            if time.time() >= deadline:
                return False
            chunk = ""
            try:
                chunk = self.pty.read(blocking=False)
            except winpty.WinptyError:
                pass
            if chunk:
                self.buf += chunk
            else:
                time.sleep(0.03)

    def pump(self, seconds):
        deadline = time.time() + seconds
        while time.time() < deadline:
            chunk = ""
            try:
                chunk = self.pty.read(blocking=False)
            except winpty.WinptyError:
                pass
            if chunk:
                self.buf += chunk
            time.sleep(0.03)


def unlock_check(exe, cfg, password):
    env = base_env(cfg)
    env["AGENT2SSH_MASTER_PASSWORD"] = password
    proc = subprocess.run([exe, "secrets", "status", "--json"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    try:
        payload = json.loads(proc.stdout.decode("utf-8", "replace"))
    except json.JSONDecodeError:
        return None
    return bool(payload.get("unlocked"))


def run_case(exe, tag, actions, verify_password=None):
    """actions: [("expect", text)] / [("write", text)] / [("wait", seconds)]"""
    cfg = cfg_dir(tag)
    env = base_env(cfg)
    # pywinpty takes a Windows environment block string, not a dict.
    env_block = "\0".join(f"{key}={value}" for key, value in env.items()) + "\0"
    pty = winpty.PTY(100, 30, backend=winpty.Backend.ConPTY)
    pty.spawn('"' + exe + '"', cmdline="secrets set-password", env=env_block)
    reader = Reader(pty)
    reached = True
    for kind, value in actions:
        if kind == "expect":
            if not reader.expect(value):
                reached = False
                reader.buf += "\n<<<TIMEOUT waiting for " + repr(value) + ">>>"
                break
        elif kind == "write":
            pty.write(value)
            reader.pump(0.6)
        else:
            reader.pump(value)
    # Tail: let the process exit on its own.
    deadline = time.time() + 20
    while time.time() < deadline and pty.isalive():
        reader.pump(0.2)
    transcript = reader.buf
    exit_status = None
    try:
        exit_status = pty.get_exitstatus()
    except Exception:
        pass
    try:
        pty.cancel_io()
    except Exception:
        pass
    unlocked = unlock_check(exe, cfg, verify_password) if verify_password else None
    echoed = (verify_password in transcript) if verify_password else None
    row = {
        "case": tag,
        "backend": "ConPTY",
        "reached_prompt": reached,
        "password_in_transcript": echoed,
        "unlock_with_expected": unlocked,
        "exit_status": exit_status,
        "transcript": transcript,
        "config_dir": cfg,
    }
    results.append(row)
    print(json.dumps({k: row[k] for k in
                      ("case", "backend", "reached_prompt", "password_in_transcript",
                       "unlock_with_expected", "exit_status")}, ensure_ascii=False))
    return row


def main():
    exe = os.path.abspath(sys.argv[1])
    outdir = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else "."

    secret = "SYNTHETIC_PTY_PASSWORD_9Q"
    run_case(exe, "conpty-plain", [
        ("expect", "New master password:"),
        ("write", secret + "\r"),
        ("expect", "Confirm master password:"),
        ("write", secret + "\r"),
    ], verify_password=secret)

    # Backspace deletes one character; the sentinel X must be gone and the
    # resulting password must still unlock the store.
    run_case(exe, "conpty-backspace", [
        ("expect", "New master password:"),
        ("write", "abX\x7fc\r"),
        ("expect", "Confirm master password:"),
        ("write", "abc\r"),
    ], verify_password="abc")

    # Non-ASCII: the terminal writes UTF-8 bytes; the CLI stores characters.
    cjk = "合成口令-测试"
    run_case(exe, "conpty-cjk", [
        ("expect", "New master password:"),
        ("write", cjk + "\r"),
        ("expect", "Confirm master password:"),
        ("write", cjk + "\r"),
    ], verify_password=cjk)

    # Ctrl+C in raw mode arrives as 0x03: the CLI must cancel, not die silently
    # and not leave a half-initialised store.
    run_case(exe, "conpty-ctrl-c", [
        ("expect", "New master password:"),
        ("write", "\x03"),
    ], verify_password=None)

    with open(os.path.join(outdir, "raw-pty-probe.json"), "w", encoding="utf-8") as handle:
        json.dump(results, handle, ensure_ascii=False, indent=2)
    print("wrote", os.path.join(outdir, "raw-pty-probe.json"))


if __name__ == "__main__":
    main()
