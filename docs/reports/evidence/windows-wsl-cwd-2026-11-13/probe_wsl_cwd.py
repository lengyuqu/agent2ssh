#!/usr/bin/env python3
"""Issue #27 follow-up — what `pwd` does a `local:wsl:...` host start in?

`local_exec.rs::wsl_plan` builds `wsl.exe [-d <distro>] --shell-type login sh -c
<command>` and deliberately leaves `cwd` unset, so the distribution inherits
whatever Windows working directory the daemon (or CLI) happens to have. This
records what that means for each starting directory, including the case a
person hits from a PowerShell prompt sitting on a UNC path.

Synthetic only: no repository file is read by the commands, and nothing outside
the printed text is written.
"""

import json
import os
import subprocess
import sys

WSL = os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32", "wsl.exe")
COMMAND = "pwd; printf 'env_pwd=%s\\n' \"$PWD\""


def run(name, distro, cwd):
    argv = [WSL]
    if distro:
        argv += ["-d", distro]
    argv += ["--shell-type", "login", "sh", "-c", COMMAND]
    try:
        proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=60)
        return {
            "case": name,
            "distro_arg": distro,
            "cwd": cwd,
            "argv": argv,
            "returncode": proc.returncode,
            "stdout": proc.stdout.strip(),
            "stderr": proc.stderr.strip(),
        }
    except OSError as error:
        return {
            "case": name,
            "distro_arg": distro,
            "cwd": cwd,
            "argv": argv,
            "error": str(error),
        }


def main():
    outdir = sys.argv[1] if len(sys.argv) > 1 else "."
    listing = subprocess.run([WSL, "-l", "-v"], capture_output=True, text=True)
    # wsl.exe writes UTF-16LE on some builds; normalise whatever came back.
    listing_out = listing.stdout
    if "\x00" in listing_out:
        listing_out = listing_out.replace("\x00", "")

    cases = [
        run("default-distro-cwd-user", None, os.path.expanduser("~")),
        run("default-distro-cwd-system32", None, os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32")),
        run("default-distro-cwd-drive-root", None, "D:\\"),
        run("named-distro-cwd-user", "Ubuntu", os.path.expanduser("~")),
        # A UNC path *can* be a process working directory (`CreateProcess`
        # accepts one), so ask what `wsl.exe` makes of it rather than assuming
        # the caller falls back to the Windows directory.
        run("unc-cwd-localhost", None, r"\\localhost\C$\Users"),
        run("unc-cwd-wsl-localhost", None, r"\\wsl.localhost\Ubuntu\home"),
        run("unc-shell-fallback-cwd", None, os.path.join(os.environ.get("SystemRoot", r"C:\Windows"))),
    ]
    payload = {
        "wsl_list": listing_out.strip(),
        "cases": cases,
    }
    print(json.dumps(payload, ensure_ascii=False, indent=2))
    with open(os.path.join(outdir, "raw-wsl-cwd.json"), "w", encoding="utf-8") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)


if __name__ == "__main__":
    main()
