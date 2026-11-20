"""Validate the M11.7 clean-user-setup evidence and its documented scope."""

import hashlib
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_DIR = ROOT / "docs" / "testing" / "evidence" / "2026-11-19-clean-setup"
EVIDENCE = EVIDENCE_DIR / "evidence.json"
EVIDENCE_SHA256 = "80bf039346869f6e7139ce3be9539048e76c5cdfc59f4540110dbfadffb0625f"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
TARGET_VERSION = "1.21.4"
TARGET_DATA_VERSION = 4189


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class CleanSetupEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = EVIDENCE.read_bytes()
        cls.evidence = json.loads(cls.raw.decode("utf-8"))
        cls.checks = cls.evidence["checks"]

    def test_published_evidence_has_a_stable_digest(self):
        self.assertEqual(hashlib.sha256(self.raw).hexdigest(), EVIDENCE_SHA256)
        self.assertTrue((EVIDENCE_DIR / "README.md").is_file())

    def test_records_the_supported_target(self):
        target = self.evidence["target"]
        self.assertEqual(target["minecraftVersion"], TARGET_VERSION)
        self.assertEqual(target["dataVersion"], TARGET_DATA_VERSION)
        self.assertEqual(target["javaMajor"], 21)

    def test_environment_is_a_real_isolated_desktop_run(self):
        environment = self.evidence["environment"]
        self.assertEqual(environment["platform"], "Windows")
        self.assertEqual(environment["appVersion"], "0.1.0")
        self.assertEqual(
            environment["executable"], "target/debug/furybuilder-desktop.exe"
        )
        self.assertRegex(environment["executableSha256"], SHA256)
        self.assertTrue(environment["isolatedAppData"])
        self.assertEqual(environment["isolatedWebView2Profiles"], 2)
        self.assertTrue(environment["sanitizedPathVariable"])
        self.assertTrue(environment["javaHomeRemoved"])
        self.assertEqual(environment["launcherRootsInCleanSession"], 0)
        self.assertIn("closed loopback proxy", environment["offlineMode"])
        # The host may have network; the evidence must say so instead of claiming offline.
        self.assertIsInstance(environment["networkReachable"], bool)

    def test_clean_session_found_no_java_or_minecraft(self):
        clean = self.evidence["cleanStart"]
        self.assertFalse(clean["assets"]["defaultRootPresent"])
        self.assertEqual(clean["assets"]["installations"], 0)
        self.assertTrue(clean["assets"]["problemsMentionMissingLauncher"])
        self.assertEqual(clean["java"]["candidates"], 0)
        self.assertEqual(clean["java"]["compatible"], 0)
        self.assertEqual(clean["java"]["managed"], 0)
        self.assertEqual(clean["java"]["managedBytes"], 0)
        self.assertTrue(clean["java"]["problemsMentionNoJava"])
        self.assertIsNone(clean["server"]["selected"])
        self.assertEqual(clean["server"]["managed"], 0)
        self.assertFalse(clean["vanillaSetup"]["ready"])
        self.assertFalse(clean["vanillaSetup"]["javaReady"])
        self.assertFalse(clean["vanillaSetup"]["serverReady"])
        self.assertFalse(clean["vanillaSetup"]["eulaMatchesSelected"])
        self.assertGreaterEqual(clean["vanillaSetup"]["blockerCount"], 2)
        self.assertEqual(clean["managedCaches"]["javaEntries"], 0)
        self.assertEqual(clean["managedCaches"]["serverEntries"], 0)
        self.assertEqual(clean["managedCaches"]["javaFiles"], [])
        self.assertEqual(clean["managedCaches"]["serverFiles"], [])
        self.assertEqual(clean["managedCaches"]["runFiles"], [])

    def test_empty_project_was_saved_and_reopened_without_dependencies(self):
        empty = self.evidence["emptyProject"]
        self.assertTrue(empty["created"])
        self.assertTrue(empty["saved"])
        self.assertTrue(empty["reopened"])
        self.assertEqual(empty["creationMode"], "empty")
        self.assertEqual(empty["targetMinecraftVersion"], TARGET_VERSION)
        self.assertEqual(empty["targetDataVersion"], TARGET_DATA_VERSION)
        self.assertEqual(empty["worldRevision"], 1)
        self.assertEqual(empty["chunks"], 0)
        self.assertEqual(empty["sections"], 0)
        self.assertEqual(empty["blocks"], 0)
        self.assertEqual(empty["manifestAfterReopen"]["creationMode"], "empty")
        self.assertEqual(empty["manifestAfterReopen"]["lastSaveKind"], "manual")

    def test_settings_and_wizard_offer_explicit_dependencies(self):
        settings = self.evidence["settings"]
        self.assertTrue(settings["optionalForEmptyProject"])
        self.assertEqual(settings["readiness"], "setup_required")
        self.assertTrue(settings["mentionsNeverDownloads"])

        wizard = self.evidence["vanillaConsent"]["wizard"]
        self.assertTrue(wizard["setupIncomplete"])
        self.assertFalse(wizard["ready"])
        self.assertTrue(wizard["noCompatibleJava"])
        self.assertTrue(wizard["javaNotSelected"])
        self.assertTrue(wizard["serverNotSelected"])
        self.assertTrue(wizard["eulaRequired"])
        self.assertTrue(wizard["generateDisabled"])
        self.assertTrue(wizard["consentPresent"])
        self.assertFalse(wizard["consentChecked"])
        self.assertFalse(wizard["eulaChecked"])
        self.assertTrue(wizard["downloadServerOffered"])
        self.assertTrue(wizard["chooseJavaOffered"])
        self.assertTrue(wizard["chooseServerOffered"])
        self.assertEqual(
            self.evidence["vanillaConsent"]["dependenciesAfterWizard"]["javaFiles"],
            [],
        )
        self.assertEqual(
            self.evidence["vanillaConsent"]["dependenciesAfterWizard"]["serverFiles"],
            [],
        )
        self.assertEqual(
            self.evidence["vanillaConsent"]["dependenciesAfterWizard"]["runFiles"], []
        )

    def test_clean_backend_refused_generation_without_java(self):
        backend = self.evidence["vanillaConsent"]["cleanBackend"]
        self.assertTrue(backend["rejected"])
        self.assertIn("Select a compatible Java 21 runtime", backend["message"])
        self.assertIsNone(backend["activeRun"])

    def test_offline_requests_failed_without_downloads(self):
        offline = self.evidence["offline"]
        self.assertEqual(offline["mode"], "closed loopback proxy")
        self.assertEqual(len(offline["metadataRequests"]), 2)
        self.assertTrue(all(not request["ok"] for request in offline["metadataRequests"]))
        for request in offline["metadataRequests"]:
            self.assertIn("connection", request["message"].lower())
        self.assertEqual(offline["dependencyFilesAfter"], [])

    def test_launcher_layouts_were_discovered_read_only(self):
        layouts = self.evidence["launcherLayouts"]
        synthetic = layouts["synthetic"]
        self.assertTrue(synthetic["defaultRoot"])
        self.assertEqual(synthetic["profileGameDirs"], 2)
        self.assertEqual(synthetic["installations"], 3)
        self.assertEqual(synthetic["sources"].count("default_root"), 1)
        self.assertEqual(synthetic["sources"].count("launcher_profile"), 2)
        self.assertTrue(
            all(versions == [TARGET_VERSION] for versions in synthetic["compatibleVersions"])
        )
        self.assertTrue(
            all(
                statuses == ["compatible"]
                for statuses in synthetic["targetVersionsVerified"]
            )
        )
        self.assertTrue(synthetic["launcherRootsUnchanged"])
        self.assertEqual(synthetic["changedFiles"], [])

        real = layouts["realThirdPartyRoot"]
        self.assertIsNotNone(real)
        self.assertTrue(real["present"])
        self.assertTrue(real["valid"])
        self.assertTrue(real["targetMatch"])
        self.assertEqual(real["kind"], "minecraft_root")
        self.assertGreaterEqual(real["versionCount"], 1)

    def test_damaged_and_stale_caches_were_rejected(self):
        cache = self.evidence["corruptedCache"]
        java = cache["managedJava"]
        self.assertEqual(java["entries"], 1)
        self.assertEqual(java["compatible"], 0)
        self.assertEqual(java["statuses"], {"unreadable": 2})
        self.assertGreaterEqual(java["candidateCount"], 1)
        server = cache["managedServer"]
        self.assertEqual(server["entries"], 0)
        self.assertTrue(server["problemsMentionChangedOnDisk"])
        self.assertEqual(cache["staleJavaSelection"]["savedStatus"], "unreadable")
        self.assertIn("cannot find", cache["staleJavaSelection"]["savedError"])
        self.assertFalse(cache["staleServerSelection"]["ready"])
        self.assertIn("cannot find", cache["staleServerSelection"]["error"])
        self.assertFalse(cache["vanillaSetupReady"])
        self.assertEqual(cache["dependencyFiles"]["runs"], [])

    def test_consent_gate_blocks_before_the_server_jar(self):
        gate = self.evidence["corruptedCache"]["consentGate"]
        self.assertEqual(gate["selectedJavaStatus"], "compatible")
        self.assertTrue(gate["withoutConsent"]["rejected"])
        self.assertIn(
            "explicit generation consent is required",
            gate["withoutConsent"]["message"],
        )
        self.assertTrue(gate["withConsentButMissingServer"]["rejected"])
        self.assertIn("server JAR", gate["withConsentButMissingServer"]["message"])
        self.assertIsNone(gate["activeRun"])

    def test_every_recorded_check_passed(self):
        self.assertTrue(self.evidence["pass"])
        self.assertEqual(len(self.checks), 25)
        self.assertTrue(all(value is True for value in self.checks.values()))
        self.assertEqual(self.evidence["consoleErrors"], [])

    def test_limitations_are_explicit(self):
        limitations = " ".join(self.evidence["limitations"])
        self.assertIn("not a fresh Windows installation", limitations)
        self.assertIn("launcher brands", limitations)
        self.assertIn("closed loopback proxy", limitations)
        self.assertIn("KL-43", limitations)
        self.assertIn("synthetic", limitations)

    def test_no_absolute_paths_secrets_or_mojang_binaries_leak(self):
        combined = "\n".join(
            [
                self.raw.decode("utf-8"),
                read(EVIDENCE_DIR / "README.md"),
                read(ROOT / "docs" / "testing" / "clean-setup.md"),
            ]
        )
        self.assertNotIn("C:\\Users\\", combined)
        self.assertNotIn("D:\\mitrium", combined)
        self.assertNotIn("\\\\pipe", combined)
        self.assertNotIn("token=", combined.lower())
        self.assertNotIn("bearer ", combined.lower())
        self.assertNotIn("-----BEGIN", combined)
        self.assertNotIn("data:application", combined)
        self.assertNotIn("UEsDB", combined)  # base64 ZIP/JAR signature

    def test_documentation_and_harness_are_linked(self):
        doc = read(ROOT / "docs" / "testing" / "clean-setup.md")
        for expected in (
            "M11.7",
            "1.21.4",
            "evidence/2026-11-19-clean-setup/evidence.json",
            "scripts/clean-setup-webview2.mjs",
            "closed loopback proxy",
            "KL-43",
            "not a fresh Windows installation",
        ):
            self.assertIn(expected, doc)

        matrix = read(ROOT / "docs" / "testing" / "test-matrix.md")
        self.assertIn("M11.7", matrix)
        self.assertIn("2026-11-19-clean-setup/evidence.json", matrix)
        self.assertIn("M11.7 stays unchecked", matrix)

        java_doc = read(ROOT / "docs" / "runtime" / "java-runtime.md")
        self.assertIn("HTTPS_PROXY", java_doc)
        server_doc = read(ROOT / "docs" / "runtime" / "minecraft-server-jar.md")
        self.assertIn("HTTPS_PROXY", server_doc)

        script = read(ROOT / "scripts" / "clean-setup-webview2.mjs")
        for expected in (
            "FURYBUILDER_APP_DATA_DIR",
            "JAVA_HOME",
            "HTTPS_PROXY",
            "create",
            "runtime_select_java",
            "Start Vanilla World generation",
            "Confirm local official server run",
            "server_start_generation",
            "assets_discover",
            "runtime_discover",
            "server_managed_list",
        ):
            self.assertIn(expected, script)
        self.assertNotIn("C:\\Users\\Mike", script)
        self.assertNotIn("D:\\mitrium", script)

    def test_ci_runs_the_clean_setup_validator(self):
        ci = read(ROOT / ".github" / "workflows" / "ci.yml")
        self.assertIn("python scripts/test_clean_setup.py", ci)


if __name__ == "__main__":
    unittest.main()
