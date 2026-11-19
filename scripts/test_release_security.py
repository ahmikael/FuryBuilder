"""Validate the M11 release security evidence, license policy and notices.

The installer and dependency scans are executed by external tools on a real machine.
These tests keep the recorded evidence honest and make the documented policy and the
repository configuration fail together when they drift.
"""

import json
import re
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE = ROOT / "docs" / "testing" / "evidence"
SECURITY = EVIDENCE / "2026-11-19-release-security"
INSTALLER = EVIDENCE / "2026-11-19-windows-installer" / "installer-evidence.json"
INVENTORY = ROOT / "docs" / "legal" / "third-party-inventory.json"
SHA256 = re.compile(r"^[0-9a-f]{64}$")


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


class ReleaseSecurityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cargo = read_json(SECURITY / "cargo-audit.json")
        cls.pnpm_prod = read_json(EVIDENCE / "2026-11-19-release-security-pnpm-audit.json")
        cls.pnpm_all = read_json(EVIDENCE / "2026-11-19-release-security-pnpm-audit-all.json")
        cls.signature = read_json(SECURITY / "signature-evidence.json")
        cls.inventory = read_json(INVENTORY)
        cls.installer = read_json(INSTALLER)

    def test_rust_audit_has_no_vulnerabilities(self):
        self.assertIs(self.cargo["vulnerabilities"]["found"], False)
        self.assertEqual(self.cargo["vulnerabilities"]["count"], 0)
        self.assertEqual(self.cargo["vulnerabilities"]["list"], [])
        database = self.cargo["database"]
        self.assertGreater(database["advisory-count"], 1000)
        self.assertTrue(database["last-updated"].startswith("2026-11-19"))

    def test_rust_audit_warnings_are_the_documented_linux_only_set(self):
        warnings = self.cargo["warnings"]
        unmaintained = {item["advisory"]["id"] for item in warnings.get("unmaintained", [])}
        unsound = {item["advisory"]["id"] for item in warnings.get("unsound", [])}
        self.assertEqual(
            unmaintained,
            {"RUSTSEC-2024-0370", "RUSTSEC-2026-0543"},
            "unexpected unmaintained warning; update the audit assessment",
        )
        self.assertEqual(unsound, {"RUSTSEC-2024-0429"})
        packages = {
            item["package"]["name"]
            for group in warnings.values()
            if isinstance(group, list)
            for item in group
        }
        self.assertEqual(packages, {"glib", "proc-macro-error", "chrono"})

        audit_doc = (ROOT / "docs" / "legal" / "security-audit.md").read_text(encoding="utf-8")
        for identifier in sorted(unmaintained | unsound):
            self.assertIn(identifier, audit_doc)
        self.assertIn("Linux", audit_doc)
        self.assertIn("GTK3", audit_doc)

    def test_zip_advisories_were_fixed_by_the_upgrade(self):
        runtime_cargo = (ROOT / "crates" / "runtime-manager" / "Cargo.toml").read_text(
            encoding="utf-8"
        )
        self.assertIn('zip = { version = "9"', runtime_cargo)
        workspace = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn('rust-version = "1.88"', workspace)

        zip_packages = [
            item
            for item in self.inventory["rust"]["packages"]
            if item["name"] == "zip"
        ]
        self.assertTrue(zip_packages, "zip must be in the generated inventory")
        self.assertTrue(
            all(item["version"].startswith("9.") for item in zip_packages),
            f"unexpected zip versions: {zip_packages}",
        )
        audit_doc = (ROOT / "docs" / "legal" / "security-audit.md").read_text(encoding="utf-8")
        for identifier in (
            "RUSTSEC-2026-0367",
            "RUSTSEC-2026-0368",
            "RUSTSEC-2026-0369",
            "RUSTSEC-2026-0429",
            "RUSTSEC-2026-0545",
            "RUSTSEC-2026-0548",
        ):
            self.assertIn(identifier, audit_doc)
        self.assertIn("9.7.3", audit_doc)

    def test_node_audits_are_clean(self):
        for data, minimum in ((self.pnpm_prod, 70), (self.pnpm_all, 390)):
            vulnerabilities = data["metadata"]["vulnerabilities"]
            self.assertEqual(set(vulnerabilities.values()), {0})
            self.assertGreaterEqual(data["metadata"]["totalDependencies"], minimum)
            self.assertEqual(data["advisories"], {})

    def test_license_inventory_passes_the_policy(self):
        self.assertIs(self.inventory["pass"], True)
        self.assertGreaterEqual(self.inventory["rust"]["packageCount"], 400)
        self.assertGreaterEqual(self.inventory["node"]["packageCount"], 50)
        self.assertEqual(self.inventory["denied"], [])
        self.assertEqual(self.inventory["unknown"], [])
        self.assertGreaterEqual(len(self.inventory["noticeRequired"]), 20)

        licenses = {
            item["license"] for item in self.inventory["noticeRequired"]
        }
        self.assertTrue(any("MPL-2.0" in value for value in licenses))
        self.assertTrue(any("Unicode-3.0" in value for value in licenses))
        self.assertTrue(any("CDLA-Permissive-2.0" in value for value in licenses))

        policy = subprocess.run(
            ["node", str(ROOT / "scripts" / "check-dependencies.mjs"), "--check"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(policy.returncode, 0, policy.stderr)
        self.assertIn("License policy passed", policy.stdout)

    def test_generated_notices_are_committed_and_linked(self):
        markdown = (ROOT / "docs" / "legal" / "third-party-inventory.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("# Generated third-party inventory", markdown)
        self.assertIn("Do not edit it by hand", markdown)
        self.assertIn("MPL-2.0", markdown)
        self.assertIn("Unicode-3.0", markdown)

        notices = (ROOT / "docs" / "legal" / "third-party-notices.md").read_text(
            encoding="utf-8"
        )
        self.assertIn("third-party-inventory.md", notices)
        self.assertIn("security-audit.md", notices)
        self.assertIn("pnpm licenses:check", notices)
        self.assertIn("scripts/verify-release-artifacts.ps1", notices)

    def test_signature_evidence_is_honest_and_matches_the_installer_evidence(self):
        self.assertEqual(self.signature["schemaVersion"], 1)
        self.assertEqual(self.signature["milestone"], "M11")
        self.assertEqual(self.signature["task"], "release-security-signature")
        self.assertIs(self.signature["requireSigned"], False)
        self.assertIs(self.signature["pass"], True)
        self.assertEqual(self.signature["problems"], [])
        self.assertTrue(self.signature["limitations"])

        artifacts = {item["file"]: item for item in self.signature["artifacts"]}
        self.assertEqual(len(artifacts), 2)
        expected = {
            item["file"]: item for item in self.installer["installers"] if item["format"] in {"nsis", "msi"}
        }
        for name, record in artifacts.items():
            self.assertEqual(record["signatureStatus"], "NotSigned")
            self.assertIs(record["isSigned"], False)
            self.assertIsNone(record["signerSubject"])
            self.assertEqual(record["productVersion"], "0.1.0")
            self.assertRegex(record["sha256"], SHA256)
            self.assertEqual(record["sha256"], expected[name]["sha256"], name)
            self.assertEqual(record["bytes"], expected[name]["bytes"], name)
        self.assertTrue(any("Unsigned artifacts" in item for item in self.signature["limitations"]))

    def test_release_artifacts_script_enforces_signed_and_tampered_states(self):
        script = (ROOT / "scripts" / "verify-release-artifacts.ps1").read_text(encoding="utf-8")
        self.assertIn("-RequireSigned", script)
        self.assertIn("Get-AuthenticodeSignature", script)
        self.assertIn("Get-FileHash", script)
        self.assertIn('"Valid", "NotSigned"', script)
        self.assertIn("tampered or untrusted signature", script)
        self.assertIn("ProductVersion", script)
        self.assertIn("Get-MsiProductVersion", script)

        audit_doc = (ROOT / "docs" / "legal" / "security-audit.md").read_text(encoding="utf-8")
        self.assertIn("NotSigned", audit_doc)
        self.assertIn("code-signing certificate", audit_doc)
        self.assertIn("-RequireSigned", audit_doc)
        self.assertIn("SmartScreen", audit_doc)

    def test_release_checklist_and_ci_enforce_the_security_gates(self):
        package = read_json(ROOT / "package.json")
        self.assertEqual(
            package["scripts"]["licenses:check"], "node scripts/check-dependencies.mjs --check"
        )
        self.assertEqual(
            package["scripts"]["licenses:update"], "node scripts/check-dependencies.mjs"
        )
        ci = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("pnpm licenses:check", ci)

        checklist = (ROOT / "docs" / "release" / "versioning.md").read_text(encoding="utf-8")
        self.assertIn("cargo audit", checklist)
        self.assertIn("pnpm audit --prod", checklist)
        self.assertIn("verify-release-artifacts.ps1", checklist)
        self.assertIn("-RequireSigned", checklist)

    def test_no_private_key_or_certificate_material_is_committed(self):
        checked = [
            SECURITY / "signature-evidence.json",
            ROOT / "docs" / "legal" / "security-audit.md",
            ROOT / "docs" / "legal" / "third-party-notices.md",
        ]
        for path in checked:
            text = path.read_text(encoding="utf-8")
            self.assertNotIn("BEGIN PRIVATE KEY", text, path.name)
            self.assertNotIn("BEGIN CERTIFICATE", text, path.name)
        signature_bytes = (SECURITY / "signature-evidence.json").read_bytes()
        self.assertTrue(signature_bytes.startswith(b"{"), "evidence must be UTF-8 without a BOM")
        self.assertNotIn(b"\x00", signature_bytes)


if __name__ == "__main__":
    unittest.main()
