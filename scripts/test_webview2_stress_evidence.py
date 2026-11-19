"""Check integrity and recorded results of the large-map WebView2 stress run."""

import hashlib
import json
import struct
import unittest
from pathlib import Path


EVIDENCE = (
    Path(__file__).resolve().parent.parent
    / "docs"
    / "testing"
    / "evidence"
    / "2026-11-19-webview2-stress"
)
PNG_SHA256 = "91a2db32aabd4574c3b927cc328ce4e62503d4d3a658167bfd82436879756d27"


class WebViewStressEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.metrics = json.loads((EVIDENCE / "metrics.json").read_text(encoding="utf-8"))

    def test_records_the_full_stress_envelope(self):
        self.assertEqual(self.metrics["kind"], "webview2-large-map-stress")
        self.assertEqual(self.metrics["schemaVersion"], 1)
        environment = self.metrics["environment"]
        self.assertTrue(environment["webglVersion"].startswith("WebGL 2.0"))
        self.assertEqual(environment["glError"], 0)
        scene = self.metrics["scenario"]
        self.assertEqual(scene["totalChunks"], 4_225)
        self.assertEqual(scene["nonAirBlocks"], 9_022_512)
        self.assertEqual(scene["radii"], [8, 12, 16, 20, 24, 28, 32])
        self.assertEqual(scene["batchSizes"], [16, 32, 64])
        self.assertGreater(self.metrics["durationMs"], 0)

    def test_every_snapshot_loaded_completely_and_captured(self):
        snapshots = self.metrics["snapshots"]
        self.assertEqual(
            [snapshot["radius"] for snapshot in snapshots],
            [8, 12, 16, 20, 24, 28, 32],
        )
        previous_chunks = 0
        for snapshot in snapshots:
            self.assertGreater(snapshot["chunks"], previous_chunks)
            previous_chunks = snapshot["chunks"]
            self.assertIsNone(snapshot["failed"])
            self.assertGreater(snapshot["setupMs"], 0)
            self.assertEqual(snapshot["metrics"]["loadedChunks"], snapshot["chunks"])
            self.assertEqual(
                snapshot["overviewMetrics"]["loadedChunks"], snapshot["chunks"]
            )
            capture = snapshot["capture"]
            self.assertEqual(capture["worldRevision"], 1)
            self.assertEqual(capture["missingChunks"], 0)
            self.assertIs(capture["partial"], False)
        last = snapshots[-1]
        self.assertEqual(last["chunks"], 4_225)
        self.assertEqual(last["metrics"]["meshCount"], 4_225)
        self.assertGreater(last["metrics"]["triangles"], 0)
        self.assertGreater(last["overviewMetrics"]["drawCalls"], 0)

    def test_streaming_runs_reach_every_chunk(self):
        streaming = self.metrics["streaming"]
        self.assertIsNone(streaming["failed"])
        self.assertEqual(
            [run["batchChunks"] for run in streaming["runs"]], [16, 32, 64]
        )
        for run in streaming["runs"]:
            self.assertIsNone(run["failed"])
            self.assertEqual(run["metrics"]["loadedChunks"], 4_225)
            self.assertGreater(run["totalMs"], 0)
            self.assertGreater(run["updates"], 0)
            self.assertEqual(run["rings"][-1]["ring"], 32)
            self.assertLessEqual(
                max(ring["maxBatchMs"] for ring in run["rings"]), 1_000
            )

    def test_png_matches_hash_resolution_and_metrics(self):
        image = (EVIDENCE / "viewport.png").read_bytes()
        self.assertGreaterEqual(len(image), 24)
        self.assertEqual(image[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(image[12:16], b"IHDR")
        self.assertEqual(list(struct.unpack_from(">II", image, 16)), [1600, 900])
        capture = self.metrics["capture"]
        self.assertEqual(capture["pngBytes"], len(image))
        self.assertEqual(capture["pngSha256"], PNG_SHA256)
        self.assertEqual(hashlib.sha256(image).hexdigest(), PNG_SHA256)


if __name__ == "__main__":
    unittest.main()
