#!/usr/bin/env python3
"""Check a candidate macOS daemon's *static* Developer ID signing policy.

This is an explicit release-review tool, not a runtime trust anchor. Team ID
and code identifier must come from the release owner's trusted configuration,
not from the candidate binary. It does not check notarization, a running
process, Mach endpoint ownership, or the integrity of the containing app.
"""
import argparse
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
from xml.parsers.expat import ExpatError

TEAM = re.compile(r"[A-Z0-9]{10}\Z", re.ASCII)
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,254}\Z", re.ASCII)
FORBIDDEN = (
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.disable-library-validation",
    "com.apple.security.cs.allow-unsigned-executable-memory",
    "com.apple.security.cs.allow-jit",
    "com.apple.security.cs.disable-executable-page-protection",
    "com.apple.security.get-task-allow",
)


class Refused(Exception):
    pass


def _codesign(*args: str) -> subprocess.CompletedProcess[bytes]:
    env = {k: v for k, v in os.environ.items() if not k.startswith("DYLD_")}
    try:
        return subprocess.run(
            ["/usr/bin/codesign", *args], capture_output=True, timeout=30, env=env
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refused(f"cannot run codesign: {error}") from error


def verify_developer_id(path: Path, team: str, identifier: str) -> None:
    if not TEAM.fullmatch(team) or not IDENTIFIER.fullmatch(identifier):
        raise Refused("invalid expected Team ID or code identifier")
    # Apple TN3127: constrain both Apple anchor and Developer ID Application
    # certificate types as well as the explicitly expected team/identifier.
    requirement = (
        '=anchor apple generic and '
        'certificate 1[field.1.2.840.113635.100.6.2.6] exists and '
        'certificate leaf[field.1.2.840.113635.100.6.1.13] exists and '
        f'certificate leaf[subject.OU] = "{team}" and identifier "{identifier}"'
    )
    result = _codesign("--verify", "--strict", "--all-architectures", "-R", requirement, str(path))
    if result.returncode:
        raise Refused("Developer ID signature or expected identity did not verify")


def verify_runtime_and_entitlements(path: Path) -> None:
    if not path.is_file():
        raise Refused("daemon candidate must be a standalone Mach-O file")
    try:
        archs = subprocess.run(["/usr/bin/lipo", "-archs", str(path)], capture_output=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refused(f"cannot inspect Mach-O architectures: {error}") from error
    names = archs.stdout.decode("ascii", errors="replace").split()
    if archs.returncode or not names or any(not re.fullmatch(r"[A-Za-z0-9_]+", name) for name in names):
        raise Refused("Mach-O architectures are absent or unreadable")
    for arch in names:
        _verify_slice(path, arch)


def _verify_slice(path: Path, arch: str) -> None:
    signature = _codesign("-dv", "--verbose=4", "--architecture", arch, str(path))
    if signature.returncode:
        raise Refused("cannot inspect code signature flags")
    details = signature.stderr.decode("utf-8", errors="replace")
    flags = re.search(r"^CodeDirectory .*flags=0x[0-9a-fA-F]+\(([^)]*)\)", details, re.MULTILINE)
    if not flags or "runtime" not in flags.group(1).split(","):
        raise Refused("hardened runtime flag is absent or unreadable")
    result = _codesign("-d", "--architecture", arch, "--entitlements", ":-", str(path))
    if result.returncode:
        raise Refused("cannot inspect code entitlements")
    try:
        entitlements = plistlib.loads(result.stdout) if result.stdout.strip() else {}
    except (ValueError, TypeError, OverflowError, ExpatError) as error:
        raise Refused("code entitlements are malformed") from error
    if not isinstance(entitlements, dict):
        raise Refused("code entitlements are not a dictionary")
    for key in FORBIDDEN:
        if key in entitlements and entitlements[key] is not False:
            raise Refused(f"forbidden or malformed entitlement: {key}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--team-id", required=True, help="trusted expected Developer ID Team ID")
    parser.add_argument("--identifier", required=True, help="trusted expected daemon code identifier")
    args = parser.parse_args()
    if sys.platform != "darwin":
        print("refused: this check requires macOS", file=sys.stderr)
        return 2
    try:
        try:
            candidate = args.candidate.resolve(strict=True)
        except OSError as error:
            raise Refused(f"cannot resolve daemon candidate: {error}") from error
        if not candidate.is_file():
            raise Refused("daemon candidate must be a standalone Mach-O file")
        verify_developer_id(candidate, args.team_id, args.identifier)
        verify_runtime_and_entitlements(candidate)
    except Refused as error:
        print(f"refused: {error}", file=sys.stderr)
        return 1
    print("static Developer ID and restricted runtime checks passed; runtime endpoint remains unverified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
