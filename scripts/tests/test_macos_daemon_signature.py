#!/usr/bin/env python3
"""Native macOS checks of the release-review signature guard.

Optional Developer ID fixture variables name an existing signed application;
the tests inspect it read-only and never launch or modify it.
"""
import importlib.util
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check_macos_daemon_signature.py"
SPEC = importlib.util.spec_from_file_location("macos_daemon_signature", SCRIPT)
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


@unittest.skipUnless(sys.platform == "darwin", "native macOS code signing required")
class MacosDaemonSignatureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="a2s-signer-test-", dir="/tmp")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binary = self.root / "synthetic"
        subprocess.run(["/usr/bin/clang", "-x", "c", "-", "-o", self.binary],
                       input=b"int main(void) { return 0; }\n", check=True, capture_output=True)
        self.sign()

    def sign(self, entitlement=None):
        command = ["/usr/bin/codesign", "--force", "--sign", "-", "--options", "runtime", "--identifier", "org.agent2ssh.synthetic"]
        if entitlement:
            plist = self.root / "entitlements.plist"
            plist.write_bytes(plistlib.dumps({entitlement: True}))
            command += ["--entitlements", str(plist)]
        subprocess.run([*command, str(self.binary)], check=True, capture_output=True)

    def test_adhoc_is_not_developer_id(self):
        with self.assertRaises(guard.Refused):
            guard.verify_developer_id(self.binary, "AAAAAAAAAA", "org.agent2ssh.synthetic")

    def test_hardened_runtime_and_jit_policy(self):
        guard.verify_runtime_and_entitlements(self.binary)
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", str(self.binary)],
                       check=True, capture_output=True)
        with self.assertRaisesRegex(guard.Refused, "runtime"):
            guard.verify_runtime_and_entitlements(self.binary)
        self.sign("com.apple.security.cs.allow-jit")
        with self.assertRaisesRegex(guard.Refused, "allow-jit"):
            guard.verify_runtime_and_entitlements(self.binary)

    def test_malformed_expected_identity_fails_closed(self):
        for team, identifier in [("short", "org.agent2ssh"), ("AAAAAAAAAA", 'x" or true')]:
            with self.subTest(team=team, identifier=identifier), self.assertRaises(guard.Refused):
                guard.verify_developer_id(self.binary, team, identifier)

    def test_each_universal_slice_has_restricted_entitlements(self):
        slices = []
        for arch in ("arm64", "x86_64"):
            binary = self.root / arch
            subprocess.run(["/usr/bin/clang", "-arch", arch, "-x", "c", "-", "-o", binary],
                           input=b"int main(void) { return 0; }\n", check=True, capture_output=True)
            subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--options", "runtime", str(binary)],
                           check=True, capture_output=True)
            slices.append(binary)
        universal = self.root / "universal"
        def combine():
            subprocess.run(["/usr/bin/lipo", "-create", *map(str, slices), "-output", str(universal)],
                           check=True, capture_output=True)
        combine()
        guard.verify_runtime_and_entitlements(universal)
        plist = self.root / "jit.plist"
        plist.write_bytes(plistlib.dumps({"com.apple.security.cs.allow-jit": True}))
        subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--options", "runtime",
                        "--entitlements", str(plist), str(slices[1])], check=True, capture_output=True)
        combine()
        with self.assertRaisesRegex(guard.Refused, "allow-jit"):
            guard.verify_runtime_and_entitlements(universal)

    def test_optional_real_developer_id_fixture(self):
        path = os.environ.get("A2S_TEST_SIGNED_FIXTURE")
        team = os.environ.get("A2S_TEST_SIGNED_TEAM")
        identifier = os.environ.get("A2S_TEST_SIGNED_IDENTIFIER")
        if not all((path, team, identifier)):
            self.skipTest("no externally signed Developer ID fixture provided")
        guard.verify_developer_id(Path(path), team, identifier)
        with self.assertRaises(guard.Refused):
            guard.verify_developer_id(Path(path), "AAAAAAAAAA", identifier)
        with self.assertRaises(guard.Refused):
            guard.verify_developer_id(Path(path), team, "org.agent2ssh.incorrect")

    def test_optional_restricted_signed_binary(self):
        path = os.environ.get("A2S_TEST_RESTRICTED_FIXTURE")
        team = os.environ.get("A2S_TEST_RESTRICTED_TEAM")
        identifier = os.environ.get("A2S_TEST_RESTRICTED_IDENTIFIER")
        if not all((path, team, identifier)):
            self.skipTest("no restricted Developer ID executable provided")
        result = subprocess.run([sys.executable, str(SCRIPT), path, "--team-id", team,
                                 "--identifier", identifier], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("runtime endpoint remains unverified", result.stdout)

    def test_optional_mixed_signed_slices_are_rejected(self):
        first = os.environ.get("A2S_TEST_RESTRICTED_FIXTURE")
        second = os.environ.get("A2S_TEST_SECOND_FIXTURE")
        team = os.environ.get("A2S_TEST_RESTRICTED_TEAM")
        first_id = os.environ.get("A2S_TEST_RESTRICTED_IDENTIFIER")
        second_id = os.environ.get("A2S_TEST_SECOND_IDENTIFIER")
        if not all((first, second, team, first_id, second_id)):
            self.skipTest("no pair of universal Developer ID executables provided")
        arm, intel, mixed = (self.root / name for name in ("arm", "intel", "mixed"))
        subprocess.run(["/usr/bin/lipo", first, "-thin", "arm64", "-output", arm], check=True, capture_output=True)
        subprocess.run(["/usr/bin/lipo", second, "-thin", "x86_64", "-output", intel], check=True, capture_output=True)
        guard.verify_developer_id(arm, team, first_id)
        guard.verify_developer_id(intel, team, second_id)
        subprocess.run(["/usr/bin/lipo", "-create", arm, intel, "-output", mixed], check=True, capture_output=True)
        with self.assertRaises(guard.Refused):
            guard.verify_developer_id(mixed, team, first_id)


if __name__ == "__main__":
    unittest.main()
