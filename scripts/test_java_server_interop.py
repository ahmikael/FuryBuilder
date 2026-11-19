"""Validate the M11.5 official Java server import/export evidence.

The checks are independent from the desktop app. They prove that the published evidence
is internally consistent, that it records a real 1.21.4 server round trip through the app
and backend export path, and that the public docs do not hide the remaining limits.
"""

import hashlib
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EVIDENCE_DIR = ROOT / "docs" / "testing" / "evidence" / "2026-11-19-java-server-interop"
EVIDENCE = EVIDENCE_DIR / "evidence.json"
EVIDENCE_SHA256 = "144837f6a454328cfc2b5da63d316ff476b507dd2ccacbe3914341863ee95544"
SHA1 = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class JavaServerInteropEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = EVIDENCE.read_bytes()
        cls.evidence = json.loads(cls.raw.decode("utf-8"))
        cls.checks = cls.evidence["checks"]

    def test_published_evidence_has_a_stable_digest(self):
        self.assertEqual(hashlib.sha256(self.raw).hexdigest(), EVIDENCE_SHA256)
        self.assertTrue((EVIDENCE_DIR / "README.md").is_file())

    def test_records_the_only_supported_target(self):
        target = self.evidence["target"]
        self.assertEqual(target["minecraftVersion"], "1.21.4")
        self.assertEqual(target["dataVersion"], 4189)
        self.assertEqual(target["layout"], "classic_anvil")
        self.assertEqual(target["supportedVersions"], ["1.21.4"])

    def test_environment_is_a_real_isolated_desktop_run(self):
        environment = self.evidence["environment"]
        self.assertEqual(environment["platform"], "Windows")
        self.assertEqual(environment["appVersion"], "0.1.0")
        self.assertEqual(
            environment["executable"], "target/debug/furybuilder-desktop.exe"
        )
        self.assertRegex(environment["executableSha256"], SHA256)
        self.assertIn("21.0.12.1", environment["javaVersion"])
        self.assertEqual(
            environment["serverJarSha1"], "4707d00eb834b446575d89a61a11b5d548d8c001"
        )
        self.assertRegex(environment["serverJarSha1"], SHA1)
        self.assertTrue(environment["isolatedAppData"])

    def test_fixture_was_generated_by_the_official_server(self):
        fixture = self.evidence["fixture"]
        self.assertEqual(fixture["generator"], "minecraft:normal")
        self.assertEqual(fixture["seed"], "4189")
        self.assertIs(fixture["generateStructures"], False)
        self.assertIn("1.21.4", fixture["serverVersionLine"])
        self.assertIn("Done (", fixture["doneLine"])
        self.assertEqual(fixture["fileCount"], 21)
        self.assertEqual(fixture["bytes"], 6_286_738)
        self.assertIs(fixture["generatedByOfficialServer"], True)
        self.assertIs(fixture["sourceUnchangedAfterImport"], True)

    def test_app_imported_the_world_as_an_immutable_import_project(self):
        app_import = self.evidence["appImport"]
        analysis = app_import["analysis"]
        self.assertIs(analysis["supported"], True)
        self.assertEqual(analysis["versionName"], "1.21.4")
        self.assertEqual(analysis["dataVersion"], 4189)
        self.assertEqual(analysis["layout"], "classic_anvil")
        self.assertEqual(analysis["chunks"], 1_416)
        self.assertEqual(analysis["regions"], 4)
        self.assertEqual(analysis["blockers"], [])

        project = app_import["project"]
        self.assertEqual(project["creationMode"], "import")
        self.assertEqual(project["targetMinecraftVersion"], "1.21.4")
        self.assertEqual(project["targetDataVersion"], 4189)
        self.assertEqual(project["worldRevision"], 1)
        self.assertTrue(project["sourceImmutable"])
        self.assertEqual(project["sourceKind"], "imported")

        world = app_import["world"]
        self.assertEqual(world["revision"], 1)
        self.assertEqual(world["chunks"], 1_416)
        self.assertEqual(world["sections"], 33_984)
        self.assertEqual(world["blocks"], 6_490_689)
        self.assertEqual(app_import["blockAtEditPositionBefore"], "minecraft:air")
        self.assertEqual(app_import["sourceCopy"], {"files": 20, "bytes": 6_286_735})

    def test_app_edit_was_saved_and_survived_reopen(self):
        edit = self.evidence["appEdit"]
        self.assertEqual(edit["operation"], "set_block")
        self.assertEqual(edit["dimension"], "minecraft:overworld")
        self.assertEqual(edit["position"], {"x": 0, "y": 300, "z": 0})
        self.assertEqual(edit["state"], "minecraft:gold_block")
        self.assertEqual(edit["changedBlocks"], 1)
        self.assertEqual(edit["revisionAfterApply"], 2)
        self.assertEqual(edit["blockAfterApply"], "minecraft:gold_block")
        self.assertEqual(edit["savedWorldRevision"], 2)
        self.assertEqual(edit["reopenedWorldRevision"], 2)
        self.assertEqual(edit["blockAfterReopen"], "minecraft:gold_block")
        self.assertEqual(edit["manifestAfterSave"]["lastSaveKind"], "manual")

    def test_export_is_a_complete_revalidated_new_world_copy(self):
        export = self.evidence["export"]
        self.assertEqual(export["backend"], "world_export_probe")
        self.assertEqual(export["derivedDataPolicy"], "vanillaRebuild")
        report = export["report"]
        self.assertEqual(report["files"], 20)
        self.assertEqual(report["bytes"], 6_286_735)
        self.assertEqual(report["regions"], 4)
        self.assertEqual(report["chunks"], 1_416)
        self.assertEqual(report["replacedRegions"], 1)
        self.assertEqual(report["replacedChunks"], 1)
        self.assertTrue(report["copiedFilesMatchSource"])
        self.assertTrue(report["outputRevalidated"])
        self.assertTrue(report["sourceUnchanged"])
        self.assertEqual(len(report["exportedRegions"]), 1)
        region = report["exportedRegions"][0]
        self.assertEqual(region["dimension"], "minecraft:overworld")
        self.assertEqual((region["regionX"], region["regionZ"]), (0, 0))
        self.assertEqual(region["bytes"], 835_584)
        self.assertEqual(region["replacedChunks"], 1)
        self.assertRegex(region["sha256"], SHA256)
        self.assertTrue(export["sourceTreeUnchanged"])
        self.assertEqual(export["exportedTree"], {"fileCount": 20, "bytes": 6_286_735})
        self.assertEqual(export["verifiedBlocks"][0]["state"], "minecraft:gold_block")
        self.assertEqual(export["verifiedBlocks"][0]["y"], 300)

    def test_official_server_loaded_and_verified_the_export(self):
        verification = self.evidence["serverVerification"]
        self.assertIn("1.21.4", verification["versionLine"])
        self.assertIn("Done (", verification["doneLine"])
        self.assertEqual(verification["exitCode"], 0)
        self.assertTrue(verification["editMarker"])
        self.assertTrue(verification["staleAirMarkerAbsent"])
        self.assertTrue(verification["neighborAirMarker"])
        self.assertTrue(verification["terrainMarker"])
        self.assertEqual(verification["issueLines"], [])
        # The only recorded server error is the non-fatal offline Yggdrasil key fetch.
        self.assertTrue(
            all(
                "Failed to request yggdrasil public key" in line
                for line in verification["errorLines"]
            ),
            verification["errorLines"],
        )
        self.assertGreaterEqual(verification["filesChangedByServer"], 1)
        changed = set(verification["changedFiles"])
        self.assertIn("level.dat", changed)
        self.assertIn("region/r.0.0.mca", changed)
        self.assertIn("session.lock", changed)

    def test_app_reimported_the_server_saved_export(self):
        reimport = self.evidence["reimport"]
        self.assertIs(reimport["analysis"]["supported"], True)
        self.assertEqual(reimport["analysis"]["versionName"], "1.21.4")
        self.assertEqual(reimport["analysis"]["dataVersion"], 4189)
        self.assertEqual(reimport["analysis"]["chunks"], 1_416)
        self.assertEqual(reimport["project"]["creationMode"], "import")
        self.assertEqual(reimport["world"]["chunks"], 1_416)
        self.assertGreater(reimport["world"]["blocks"], 6_000_000)
        self.assertEqual(reimport["editedBlock"], "minecraft:gold_block")
        self.assertEqual(reimport["terrainBlock"], "minecraft:bedrock")

    def test_every_recorded_check_passed(self):
        self.assertIs(self.evidence["pass"], True)
        self.assertEqual(len(self.checks), 21)
        self.assertTrue(all(value is True for value in self.checks.values()))
        self.assertEqual(self.evidence["consoleErrors"], [])

    def test_limitations_are_explicit(self):
        limitations = " ".join(self.evidence["limitations"])
        self.assertIn("1.21.4", limitations)
        self.assertIn("client was not launched", limitations)
        self.assertIn("block-entity", limitations)
        self.assertIn("KL-10", limitations)
        self.assertIn("not distributed", limitations)

    def test_no_absolute_paths_tokens_or_server_binaries_leak(self):
        combined = "\n".join(
            [
                self.raw.decode("utf-8"),
                read(EVIDENCE_DIR / "README.md"),
                read(ROOT / "docs" / "testing" / "java-server-interop.md"),
            ]
        )
        self.assertNotIn("C:\\Users\\", combined)
        self.assertNotIn("D:\\mitrium", combined)
        self.assertNotIn("\\\\pipe", combined)
        self.assertNotIn("token", combined.lower())
        self.assertNotIn("-----BEGIN", combined)
        self.assertNotIn("server.jar", EVIDENCE.read_text(encoding="utf-8"))

    def test_documentation_and_harness_are_linked(self):
        testing_doc = read(ROOT / "docs" / "testing" / "java-server-interop.md")
        for expected in (
            "M11.5",
            "1.21.4",
            "evidence/2026-11-19-java-server-interop/evidence.json",
            "scripts/java-server-interop-webview2.mjs",
            "--accept-minecraft-eula",
            "KL-10",
        ):
            self.assertIn(expected, testing_doc)

        matrix = read(ROOT / "docs" / "testing" / "test-matrix.md")
        self.assertIn("M11.5", matrix)
        self.assertIn("2026-11-19-java-server-interop/evidence.json", matrix)

        support = read(ROOT / "docs" / "formats" / "support-matrix.md")
        self.assertIn("2026-11-19-java-server-interop/evidence.json", support)

        export_doc = read(ROOT / "docs" / "formats" / "java-world-export.md")
        self.assertIn("2026-11-19-java-server-interop/evidence.json", export_doc)

        changelog = read(ROOT / "CHANGELOG.md")
        self.assertIn("official 1.21.4 server", changelog)

        script = read(ROOT / "scripts" / "java-server-interop-webview2.mjs")
        for expected in (
            "--accept-minecraft-eula",
            "importWorld",
            "applyEditorOperation",
            "world_export_probe",
            "FURY_INTEROP_EDIT_OK",
            "FURY_INTEROP_NEIGHBOR_AIR_OK",
            "FURY_INTEROP_STALE_AIR",
        ):
            self.assertIn(expected, script)
        self.assertNotIn("C:\\Users\\Mike", script)
        self.assertNotIn("D:\\mitrium", script)


if __name__ == "__main__":
    unittest.main()
