"""Validate the M10.8 long-editing evidence recorded on Windows graphics paths."""

import hashlib
import json
import struct
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
EVIDENCE = ROOT / "docs" / "testing" / "evidence" / "2026-11-19-long-edit-gpu"
RUNS = {
    "hardware-d3d11": {
        "min_duration_ms": 600_000,
        "renderer_terms": ["NVIDIA GeForce GTX 1060", "D3D11"],
        "min_samples": 40,
    },
    "software-warp": {
        "min_duration_ms": 300_000,
        "renderer_terms": ["Microsoft Basic Render Driver"],
        "min_samples": 20,
    },
    "software-swiftshader": {
        "min_duration_ms": 300_000,
        "renderer_terms": ["SwiftShader"],
        "min_samples": 20,
    },
}


class LongEditEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.runs = {}
        for name, config in RUNS.items():
            path = EVIDENCE / "runs" / name / "metrics.json"
            self.assertTrue(path.is_file(), f"missing evidence: {path}")
            run = json.loads(path.read_text(encoding="utf-8"))
            run["_path"] = path
            run["_config"] = config
            self.runs[name] = run

    def test_every_run_completed_a_real_long_editing_session(self):
        for name, run in self.runs.items():
            config = run["_config"]
            self.assertEqual(run["kind"], "long-edit-webview2")
            self.assertEqual(run["schemaVersion"], 1)
            self.assertEqual(run["milestone"], "M10.8")
            self.assertIs(run["pass"], True, name)
            self.assertIsNone(run["failure"], name)
            self.assertGreaterEqual(run["durationMs"], config["min_duration_ms"], name)

            operations = run["operations"]
            self.assertGreaterEqual(operations["attempted"], 100, name)
            self.assertEqual(operations["succeeded"], operations["attempted"], name)
            self.assertEqual(operations["failed"], 0, name)
            self.assertGreaterEqual(operations["changedBlocks"], 10_000, name)
            self.assertEqual(operations["verificationFailures"], [], name)
            self.assertGreaterEqual(len(operations["undoRedoProbes"]), 3, name)
            for probe in operations["undoRedoProbes"]:
                self.assertIs(probe["ok"], True, f"{name} probe {probe['cycle']}")
                self.assertIs(probe["targetShown"]["ok"], True, name)
                self.assertIs(probe["restored"]["ok"], True, name)
                self.assertIs(probe["targetRestored"]["ok"], True, name)
            self.assertGreaterEqual(
                len(operations["byKind"]),
                3,
                f"{name} should exercise several operation kinds",
            )

            self.assertEqual(run["revisionMismatches"], 0, name)
            self.assertEqual(run["contextLossCount"], 0, name)
            self.assertIs(run["rendererStable"], True, name)
            self.assertTrue(all(run["summary"]["assertions"].values()), name)

    def test_rendered_blocks_match_the_rust_core_after_each_edit(self):
        for name, run in self.runs.items():
            samples = run["samples"]
            self.assertGreaterEqual(len(samples), run["_config"]["min_samples"], name)
            for sample in samples:
                self.assertIs(sample["renderer"]["present"], True, name)
                self.assertIs(sample["renderer"]["sameInstance"], True, name)
                self.assertIs(sample["renderer"]["contextLost"], False, name)
                self.assertEqual(
                    sample["renderer"]["revision"],
                    sample["world"]["revision"],
                    f"{name} renderer revision at {sample['label']}",
                )
                self.assertEqual(
                    sample["rust"]["revision"],
                    sample["world"]["revision"],
                    f"{name} Rust revision at {sample['label']}",
                )
                self.assertEqual(sample["world"]["status"], "ready", name)
                self.assertIsNotNone(sample["renderer"]["metrics"], name)
                self.assertIsNotNone(sample["memory"], name)
                self.assertGreaterEqual(sample["memory"]["usedBytes"], 0, name)
                self.assertEqual(sample["observers"]["errorCount"], 0, name)
                self.assertEqual(sample["observers"]["contextLossCount"], 0, name)

            self.assertIn("fill_cuboid", run["operations"]["byKind"], name)
            self.assertIn("batch", run["operations"]["byKind"], name)
            self.assertIn("terrain_brush", run["operations"]["byKind"], name)

    def test_three_distinct_windows_graphics_paths_are_recorded(self):
        renderer_strings = set()
        for name, run in self.runs.items():
            webgl = run["app"]["webgl"]
            self.assertTrue(webgl["webglVersion"].startswith("WebGL 2.0"), name)
            self.assertIs(webgl["contextLost"], False, name)
            self.assertEqual(webgl["error"], 0, name)
            self.assertEqual(run["backend"]["id"], name)
            self.assertEqual(run["backend"]["kind"], "hardware" if name == "hardware-d3d11" else "software")
            for term in run["_config"]["renderer_terms"]:
                self.assertIn(term, webgl["renderer"], name)
            renderer_strings.add(webgl["renderer"])

            environment = run["environment"]
            self.assertTrue(environment["physicalGpuLimitation"], name)
            self.assertIn("No integrated or second discrete GPU", environment["physicalGpuLimitation"], name)
            adapters = environment["system"]["adapters"]
            if isinstance(adapters, dict):
                adapters = [adapters]
            self.assertTrue(
                any("NVIDIA GeForce GTX 1060 3GB" in (adapter.get("Name") or "") for adapter in adapters),
                name,
            )

        self.assertEqual(len(renderer_strings), 3)
        self.assertTrue(any("NVIDIA" in renderer for renderer in renderer_strings))
        self.assertTrue(any("Microsoft Basic Render Driver" in renderer for renderer in renderer_strings))
        self.assertTrue(any("SwiftShader" in renderer for renderer in renderer_strings))

    def test_save_reopen_and_screenshots_are_intact(self):
        for name, run in self.runs.items():
            save = run["summary"]["saveReopen"]
            self.assertIs(save["matches"], True, name)
            detailed = run["saveReopen"]
            self.assertIs(detailed["saved"], True, name)
            self.assertIs(detailed["closed"], True, name)
            self.assertIs(detailed["opened"], True, name)
            self.assertEqual(detailed["before"]["revision"], detailed["after"]["revision"], name)
            self.assertEqual(detailed["before"]["blockCount"], detailed["after"]["blockCount"], name)
            self.assertEqual(detailed["before"]["chunkCount"], detailed["after"]["chunkCount"], name)
            self.assertEqual(detailed["probe"]["name"], "minecraft:diamond_block", name)
            self.assertEqual(len(detailed["lastSave"]["checksum"]), 64, name)
            self.assertGreater(detailed["lastSave"]["bytesWritten"], 0, name)

            capture = run["capture"]
            self.assertEqual(capture["metadata"]["worldRevision"], detailed["after"]["revision"], name)
            for file_name, hash_key in (
                ("viewport.png", "viewportPngSha256"),
                ("app.png", "appPngSha256"),
            ):
                image = (run["_path"].parent / file_name).read_bytes()
                self.assertEqual(image[:8], b"\x89PNG\r\n\x1a\n", f"{name} {file_name}")
                self.assertEqual(image[12:16], b"IHDR", f"{name} {file_name}")
                width, height = struct.unpack_from(">II", image, 16)
                self.assertGreater(width, 0, f"{name} {file_name}")
                self.assertGreater(height, 0, f"{name} {file_name}")
                self.assertEqual(
                    hashlib.sha256(image).hexdigest(),
                    capture[hash_key],
                    f"{name} {file_name}",
                )

    def test_frame_and_heap_metrics_are_recorded_honestly(self):
        for name, run in self.runs.items():
            summary = run["summary"]
            self.assertGreater(summary["fps"]["samples"], 0, name)
            self.assertIsNotNone(summary["fps"]["median"], name)
            self.assertIsNotNone(summary["frameMs"]["p95"], name)
            self.assertGreater(summary["heap"]["initialUsedBytes"], 0, name)
            self.assertGreater(summary["heap"]["finalUsedBytes"], 0, name)
            self.assertLess(summary["heap"]["growthBytes"], 256 * 1024 * 1024, name)
            self.assertLess(summary["heap"]["maxUsedBytes"], 1536 * 1024 * 1024, name)
            if name == "hardware-d3d11":
                self.assertGreaterEqual(summary["fps"]["median"], 20, name)
            else:
                self.assertGreater(summary["fps"]["median"], 0, name)

    def test_no_console_exceptions_or_context_losses(self):
        for name, run in self.runs.items():
            self.assertEqual(run["console"]["errors"], [], name)
            self.assertEqual(run["console"]["exceptionCount"], 0, name)
            self.assertEqual(run["contextLossCount"], 0, name)
            self.assertFalse(any(sample["observers"]["errorCount"] for sample in run["samples"]), name)

    def test_evidence_readme_states_the_physical_gpu_limit(self):
        readme = (EVIDENCE / "README.md").read_text(encoding="utf-8")
        for term in (
            "M10.8",
            "GTX 1060",
            "physical",
            "WARP",
            "SwiftShader",
            "test_long_edit_evidence.py",
        ):
            self.assertIn(term, readme)


if __name__ == "__main__":
    unittest.main()
