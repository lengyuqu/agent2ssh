#!/usr/bin/env python3
"""Issue #27 — Windows real-machine probe for redirected host credentials.

Runs the built `agent2ssh` CLI through real `cmd.exe` and PowerShell pipes and
records what each shell actually feeds stdin, what the CLI stores, and what it
prints. Synthetic markers only; every run gets its own throwaway config dir.

The stored value is checked by re-reading it through the library-level
`secrets::get_secret` path from a separate helper binary (`show_secret`), so a
passing probe means "the CLI decrypted to exactly this" rather than "the file
does not contain the marker".

Usage:
    python run_windows_stdin_probe.py <path-to-agent2ssh.exe> <path-to-show_secret.exe> <outdir>
"""

import json
import os
import shutil
import subprocess
import sys
import time
import uuid

PWSH = shutil.which("pwsh") or r"C:\Program Files\PowerShell\7\pwsh.exe"
PS51 = os.path.join(
    os.environ.get("SystemRoot", r"C:\Windows"),
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe",
)

results = []


def config_dir(tag):
    path = os.path.join(
        os.environ.get("TEMP", r"C:\Windows\Temp"),
        "a2s27-" + tag + "-" + uuid.uuid4().hex[:8],
    )
    os.makedirs(path, exist_ok=True)
    return path


def base_env(cfg):
    env = os.environ.copy()
    env["AGENT2SSH_CONFIG_DIR"] = cfg
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env["AGENT2SSH_MASTER_PASSWORD"] = "probe-master-only"
    # Keep the shell's own code page / output encoding out of the comparison:
    # the CLI must not depend on either.
    return env


def run(argv, env, stdin_bytes=b"", timeout=60, shell_input=None):
    started = time.time()
    # A `list` argv goes through subprocess.list2cmdline, which escapes inner
    # quotes as \" — cmd.exe does not treat that as a quote. cmd cases are
    # therefore passed as a single command-line string and handed to
    # CreateProcess verbatim, which is what a real `cmd /c "..."` call looks
    # like to the child.
    proc = subprocess.run(
        argv,
        input=stdin_bytes if shell_input is None else shell_input,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        timeout=timeout,
    )
    return {
        "argv": argv if isinstance(argv, str) else list(argv),
        "returncode": proc.returncode,
        "stdout": proc.stdout.decode("utf-8", "replace"),
        "stderr": proc.stderr.decode("utf-8", "replace"),
        "elapsed_ms": round((time.time() - started) * 1000, 1),
    }


def stored_secret(show_exe, cfg, host):
    """Ask the helper to decrypt the stored password for `host`."""
    env = base_env(cfg)
    proc = subprocess.run(
        [show_exe, host],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    out = proc.stdout.decode("utf-8", "replace")
    # The helper prints `hex:<hex bytes>` so CR/LF and trailing spaces are
    # visible without depending on terminal rendering.
    hexed = ""
    for line in out.splitlines():
        if line.startswith("hex:"):
            hexed = line[4:].strip()
    return {
        "returncode": proc.returncode,
        "hex": hexed,
        "text": bytes.fromhex(hexed).decode("utf-8", "replace") if hexed else None,
        "stderr": proc.stderr.decode("utf-8", "replace"),
    }


def record(name, expectation, probe, secret=None, extra=None):
    row = {
        "case": name,
        "expectation": expectation,
        "returncode": probe["returncode"],
        "stdout": probe["stdout"],
        "stderr": probe["stderr"],
        "elapsed_ms": probe["elapsed_ms"],
        "argv": probe["argv"],
    }
    if secret is not None:
        row["stored_hex"] = secret["hex"]
        row["stored_text"] = secret["text"]
        row["show_rc"] = secret["returncode"]
    if extra:
        row.update(extra)
    results.append(row)
    print(json.dumps(row, ensure_ascii=False))
    return row


def main():
    exe = os.path.abspath(sys.argv[1])
    show_exe = os.path.abspath(sys.argv[2])
    outdir = os.path.abspath(sys.argv[3])
    os.makedirs(outdir, exist_ok=True)

    # ── 1. cmd.exe pipes ────────────────────────────────────────────────────
    # `echo X|` is cmd's own pipeline: it terminates the line with CRLF.
    cfg = config_dir("cmd-echo")
    cmd = 'echo SYNTHETIC_CMD_ECHO| "{}" host add cmd-echo --host 127.0.0.1 --password-stdin --json'.format(exe)
    probe = run("cmd.exe /c " + cmd, base_env(cfg))
    record("cmd-echo-pipe", "success; stored value has no CR/LF", probe, stored_secret(show_exe, cfg, "cmd-echo"), {"config_dir": cfg})

    # Redirection from a CRLF file written by cmd itself. The doubled outer
    # quotes are what cmd's `/c` heuristic needs when the executable path is
    # itself quoted and the command carries a second quoted path; without them
    # cmd returns "The filename, directory name, or volume label syntax is
    # incorrect" and never starts the CLI.
    cfg = config_dir("cmd-redirect")
    src = os.path.join(cfg, "secret-crlf.txt")
    with open(src, "wb") as handle:
        handle.write(b"SYNTHETIC_CMD_REDIRECT\r\n")
    cmd = '""{}" host add cmd-redirect --host 127.0.0.1 --password-stdin --json < "{}""'.format(exe, src)
    probe = run("cmd.exe /c " + cmd, base_env(cfg))
    record("cmd-redirect-crlf", "success; exactly one trailing CRLF trimmed", probe, stored_secret(show_exe, cfg, "cmd-redirect"), {"config_dir": cfg})

    # A file with an interior CRLF and trailing spaces must survive untouched.
    cfg = config_dir("cmd-interior")
    src = os.path.join(cfg, "secret-interior.txt")
    with open(src, "wb") as handle:
        handle.write(b"  SYNTHETIC_CMD_INTERIOR  \r\nsecond line  \r\n")
    cmd = '""{}" host add cmd-interior --host 127.0.0.1 --password-stdin --json < "{}""'.format(exe, src)
    probe = run("cmd.exe /c " + cmd, base_env(cfg))
    record("cmd-redirect-interior-lines", "success; only the final CRLF is trimmed", probe, stored_secret(show_exe, cfg, "cmd-interior"), {"config_dir": cfg})

    # Empty stdin: `type NUL |` is the cmd-shaped empty pipe.
    cfg = config_dir("cmd-empty")
    cmd = 'type NUL| "{}" host add cmd-empty --host 127.0.0.1 --password-stdin'.format(exe)
    probe = run("cmd.exe /c " + cmd, base_env(cfg))
    record("cmd-empty-pipe", "non-zero; 'secret input is empty'; no config written", probe, extra={"hosts_exists": os.path.exists(os.path.join(cfg, "hosts.json")), "config_dir": cfg})

    # Invalid UTF-8 through a cmd pipe. 0xC3 0x28 is invalid UTF-8 but is not a
    # UTF-16 BOM; a file starting with FF FE makes `type` emit nothing at all
    # (measured in probe_cmd_details.py), which would test cmd, not the CLI.
    cfg = config_dir("cmd-invalid-utf8")
    src = os.path.join(cfg, "secret-invalid.bin")
    with open(src, "wb") as handle:
        handle.write(b"\xc3\x28")
    cmd = 'type "{}"| "{}" host add cmd-invalid --host 127.0.0.1 --password-stdin'.format(src, exe)
    probe = run("cmd.exe /c " + cmd, base_env(cfg))
    record("cmd-invalid-utf8-pipe", "non-zero; stderr says UTF-8; no config written", probe, extra={"hosts_exists": os.path.exists(os.path.join(cfg, "hosts.json")), "config_dir": cfg})

    # ── 2. PowerShell pipes ────────────────────────────────────────────────
    # `"text" | native.exe` is the PowerShell 7 shape: it encodes with
    # $OutputEncoding and appends a line terminator.
    cfg = config_dir("pwsh-pipe")
    script = '& "{}" host add pwsh-pipe --host 127.0.0.1 --password-stdin --json'.format(exe)
    probe = run([PWSH, "-NoLogo", "-NoProfile", "-Command", '"SYNTHETIC_PWSH_PIPE" | ' + script], base_env(cfg))
    record("pwsh-pipe-string", "success; terminator trimmed", probe, stored_secret(show_exe, cfg, "pwsh-pipe"), {"config_dir": cfg, "shell": PWSH})

    # Same on Windows PowerShell 5.1, whose default pipe encoder on this host
    # prepends a UTF-8 BOM. The stored value shows whether the CLI strips it.
    cfg = config_dir("ps51-pipe")
    probe = run([PS51, "-NoLogo", "-NoProfile", "-Command", '"SYNTHETIC_PS51_PIPE" | ' + script.replace("pwsh-pipe", "ps51-pipe")], base_env(cfg))
    record("powershell51-pipe-string", "success; the pipe's own encoding is visible in the stored value", probe, stored_secret(show_exe, cfg, "ps51-pipe"), {"config_dir": cfg, "shell": PS51})

    # Which encoder 5.1 picked, and what a caller must set to avoid the BOM.
    cfg = config_dir("ps51-encoding")
    diag = run(
        [PS51, "-NoLogo", "-NoProfile", "-Command",
         '"type=" + $OutputEncoding.GetType().FullName; "preamble=" + $OutputEncoding.GetPreamble().Length'],
        base_env(cfg),
    )
    record("powershell51-outputencoding", "diagnostic: default pipe encoder and preamble length", diag, extra={"config_dir": cfg, "shell": PS51})

    cfg = config_dir("ps51-nobom")
    script_nobom = ('$OutputEncoding = New-Object System.Text.UTF8Encoding $false; '
                    '"SYNTHETIC_PS51_NOBOM" | & "{}" host add ps51-nobom --host 127.0.0.1 --password-stdin --json').format(exe)
    probe = run([PS51, "-NoLogo", "-NoProfile", "-Command", script_nobom], base_env(cfg))
    record("powershell51-pipe-no-bom", "explicit BOM-less encoder; stored value clean", probe, stored_secret(show_exe, cfg, "ps51-nobom"), {"config_dir": cfg, "shell": PS51})

    # `<` is not a redirection operator in PowerShell: both 7 and 5.1 must
    # reject it at parse time rather than feed it anything.
    for label, shell in (("pwsh", PWSH), ("powershell51", PS51)):
        cfg = config_dir(label + "-lt")
        cmdline = '& "{}" host add lt --host 127.0.0.1 --password-stdin < "{}"'.format(exe, os.path.join(cfg, "nope.txt"))
        probe = run([shell, "-NoLogo", "-NoProfile", "-Command", cmdline], base_env(cfg))
        record(label + "-lt-redirection", "parse error: '<' is reserved / not supported", probe, extra={"hosts_exists": os.path.exists(os.path.join(cfg, "hosts.json")), "config_dir": cfg})

    # The supported substitute: Get-Content -Raw preserves the file's own bytes
    # (plus its trailing newline), which the CLI trims once.
    cfg = config_dir("pwsh-getcontent")
    src = os.path.join(cfg, "secret.txt")
    with open(src, "wb") as handle:
        handle.write(b"SYNTHETIC_GETCONTENT\r\n")
    script = 'Get-Content -Raw -LiteralPath "{}" | & "{}" host add pwsh-getcontent --host 127.0.0.1 --password-stdin --json'.format(src, exe)
    probe = run([PWSH, "-NoLogo", "-NoProfile", "-Command", script], base_env(cfg))
    record("pwsh-getcontent-raw", "success; Get-Content -Raw adds its own terminator on top of the file's", probe, stored_secret(show_exe, cfg, "pwsh-getcontent"), {"config_dir": cfg})

    # Without -Raw the file is streamed line by line, which is the shape that
    # leaves exactly one terminator for the CLI to trim.
    cfg = config_dir("pwsh-getcontent-lines")
    script = 'Get-Content -LiteralPath "{}" | & "{}" host add pwsh-lines --host 127.0.0.1 --password-stdin --json'.format(src, exe)
    probe = run([PWSH, "-NoLogo", "-NoProfile", "-Command", script], base_env(cfg))
    record("pwsh-getcontent-lines", "success; single terminator trimmed", probe, stored_secret(show_exe, cfg, "pwsh-lines"), {"config_dir": cfg})

    # Empty input on PowerShell: $null piped to a native command sends nothing.
    cfg = config_dir("pwsh-empty")
    script = '& "{}" host add pwsh-empty --host 127.0.0.1 --password-stdin'.format(exe)
    probe = run([PWSH, "-NoLogo", "-NoProfile", "-Command", '$null | ' + script], base_env(cfg))
    record("pwsh-empty-pipe", "non-zero; 'secret input is empty'", probe, extra={"hosts_exists": os.path.exists(os.path.join(cfg, "hosts.json")), "config_dir": cfg})

    # ── 3. Non-TTY prompt path for `secrets set-password` ──────────────────
    # A pipe must not print the prompt to stderr: captured output would be
    # contaminated and the prompt is useless without a terminal.
    cfg = config_dir("setpw-pipe")
    env = base_env(cfg)
    env.pop("AGENT2SSH_MASTER_PASSWORD", None)
    script = '& "{}" secrets set-password --password SYNTHETIC_SETPW_NEW'.format(exe)
    probe = run([PWSH, "-NoLogo", "-NoProfile", "-Command", '"" | ' + script], env)
    record("set-password-pipe-no-prompt", "no 'New master password:' on stderr", probe, extra={"config_dir": cfg})

    with open(os.path.join(outdir, "raw-stdin-probe.json"), "w", encoding="utf-8") as handle:
        json.dump(results, handle, ensure_ascii=False, indent=2)
    print("\nwrote", os.path.join(outdir, "raw-stdin-probe.json"))


if __name__ == "__main__":
    main()
