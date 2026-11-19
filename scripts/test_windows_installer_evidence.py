"""Validate the M11 Windows installer evidence.

The installer itself is exercised by scripts/test-windows-installer.ps1 on a real
Windows machine. These tests make sure the recorded evidence is complete and
internally consistent, and that it does not silently drop the scenarios this
task requires.
"""

import json
import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_DIR = ROOT / "docs" / "testing" / "evidence" / "2026-11-19-windows-installer"
EVIDENCE = EVIDENCE_DIR / "installer-evidence.json"
TRANSCRIPT = EVIDENCE_DIR / "installer-test.log"
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class WindowsInstallerEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not EVIDENCE.is_file():
            raise AssertionError(f"missing installer evidence: {EVIDENCE}")
        cls.data = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        cls.scenarios = {item["name"]: item for item in cls.data["scenarios"]}
        cls.installers = cls.data["installers"]

    def test_evidence_records_a_successful_real_run(self):
        self.assertEqual(self.data["schemaVersion"], 1)
        self.assertEqual(self.data["milestone"], "M11")
        self.assertEqual(self.data["task"], "windows-installer")
        self.assertIs(self.data["pass"], True)
        self.assertIsNone(self.data["failure"])
        self.assertGreaterEqual(len(self.data["recordedAt"]), 20)
        self.assertEqual(self.data["machine"]["windows"], "Microsoft Windows 11 Pro")
        self.assertTrue(self.data["machine"]["webView2"])
        self.assertIs(self.data["machine"]["isElevated"], False)
        self.assertIs(self.data["appDataBackedUpAndRestored"], True)

    def test_every_installer_artifact_is_hashed(self):
        formats = {item["format"] for item in self.installers}
        self.assertEqual(formats, {"nsis", "msi"})
        self.assertGreaterEqual(len(self.installers), 3, "old NSIS, new NSIS and MSI are expected")
        for item in self.installers:
            self.assertGreater(item["bytes"], 1_000_000, item["file"])
            self.assertRegex(item["sha256"], SHA256, item["file"])
            self.assertRegex(item["version"], r"^\d+\.\d+\.\d+$", item["file"])

    def test_clean_install_matches_the_bundle_version(self):
        scenario = self.scenarios["nsis-clean-install"]
        self.assertIs(scenario["ok"], True)
        old = self._installer("nsis", scenario["version"])
        self.assertEqual(old["version"], scenario["version"])
        self.assertEqual(scenario["registryDisplayVersion"], scenario["version"])
        self.assertIn("FuryBuilder", scenario["installLocation"])
        self.assertIn("FuryBuilder", scenario["publisher"])

    def test_installed_application_launches_and_reports_its_version(self):
        scenario = self.scenarios["nsis-installed-app-launch"]
        self.assertIs(scenario["ok"], True)
        details = scenario["details"]
        self.assertEqual(details["windowTitle"], "FuryBuilder")
        self.assertGreater(details["windowReadySeconds"], 0)
        self.assertGreaterEqual(details["startupLogRecords"], 1)
        self.assertIs(details["versionMatched"], True)
        self.assertIs(details["closedGracefully"], True)

    def test_upgrade_replaces_the_version_and_preserves_user_data(self):
        scenario = self.scenarios["nsis-upgrade"]
        self.assertIs(scenario["ok"], True)
        self.assertFalse(scenario.get("skipped", False))
        self.assertNotEqual(scenario["fromVersion"], scenario["toVersion"])
        self.assertIs(scenario["appDataMarkerPreserved"], True)
        self.assertEqual(scenario["registryDisplayVersion"], scenario["toVersion"])
        self.assertIs(scenario["launch"]["ok"], True)
        self.assertIs(scenario["launch"]["versionMatched"], True)

    def test_uninstall_removes_program_but_keeps_user_data(self):
        scenario = self.scenarios["nsis-uninstall"]
        self.assertIs(scenario["ok"], True)
        self.assertIs(scenario["registryEntryRemoved"], True)
        self.assertIs(scenario["executableRemoved"], True)
        self.assertIs(scenario["appDataPreserved"], True)

    def test_msi_package_is_valid_even_without_elevation(self):
        scenario = self.scenarios["msi-package-extraction"]
        self.assertIs(scenario["ok"], True)
        self.assertIn("administrative extraction", scenario["mode"])
        self.assertRegex(scenario["extractedVersion"], r"^\d+\.\d+\.\d+$")
        self.assertRegex(scenario["extractedExecutableSha256"], SHA256)
        msi = self._installer("msi", scenario["extractedVersion"])
        self.assertEqual(msi["version"], scenario["extractedVersion"])
        self.assertTrue(any("elevat" in item.lower() for item in self.data["limitations"]))

    def test_transcript_has_no_hidden_failure(self):
        self.assertTrue(TRANSCRIPT.is_file())
        text = TRANSCRIPT.read_text(encoding="utf-8")
        self.assertNotIn("FAILED", text)
        self.assertIn("Clean install OK", text)
        self.assertIn("Upgrade OK", text)
        self.assertIn("Uninstall OK", text)
        self.assertIn("Installed application launch OK", text)

    def _installer(self, kind, version):
        matches = [item for item in self.installers if item["format"] == kind and item["version"] == version]
        self.assertEqual(len(matches), 1, f"expected one {kind} installer for {version}")
        return matches[0]


if __name__ == "__main__":
    unittest.main()
