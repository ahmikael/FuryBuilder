"""Check the M10.3 leak and frame-time evidence from the real WebView2 renderer."""

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
    / "2026-11-19-webview2-resilience"
)
PNG_SHA256 = "91a2db32aabd4574c3b927cc328ce4e62503d4d3a658167bfd82436879756d27"
LEAK_TOLERANCE_BYTES = 5_000_000


class WebViewResilienceEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.metrics = json.loads((EVIDENCE / "metrics.json").read_text(encoding="utf-8"))

    def test_records_the_large_map_and_gpu_identity(self):
        self.assertEqual(self.metrics["kind"], "webview2-large-map-stress")
        self.assertEqual(self.metrics["schemaVersion"], 2)
        environment = self.metrics["environment"]
        self.assertTrue(environment["webglVersion"].startswith("WebGL 2.0"))
        self.assertEqual(environment["glError"], 0)
        self.assertIn("NVIDIA GeForce GTX 1060", environment["gpuRenderer"])
        scenario = self.metrics["scenario"]
        self.assertEqual(scenario["totalChunks"], 4_225)
        self.assertEqual(scenario["nonAirBlocks"], 9_022_512)
        self.assertEqual(scenario["radii"], [8, 16, 24, 32])
        self.assertEqual(scenario["batchSizes"], [16, 32, 64])
        self.assertGreater(self.metrics["durationMs"], 0)

    def test_snapshots_are_bounded_and_complete(self):
        snapshots = self.metrics["snapshots"]
        self.assertEqual(
            [snapshot["radius"] for snapshot in snapshots], [8, 16, 24, 32]
        )
        for snapshot in snapshots:
            self.assertIsNone(snapshot["failed"])
            self.assertEqual(snapshot["metrics"]["loadedChunks"], snapshot["chunks"])
            self.assertEqual(snapshot["metrics"]["pendingMeshJobs"], 0)
            self.assertEqual(
                snapshot["overviewMetrics"]["loadedChunks"], snapshot["chunks"]
            )
            capture = snapshot["capture"]
            self.assertEqual(capture["missingChunks"], 0)
            self.assertIs(capture["partial"], False)
            # The bounded mesh queue keeps the call that installs the snapshot short;
            # the remaining work is drained over later animation frames.
            self.assertLess(snapshot["setupMs"], 2_000)

    def test_streaming_records_batch_and_frame_gaps(self):
        streaming = self.metrics["streaming"]
        self.assertIsNone(streaming["failed"])
        self.assertEqual(
            [run["batchChunks"] for run in streaming["runs"]], [16, 32, 64]
        )
        for run in streaming["runs"]:
            self.assertIsNone(run["failed"])
            self.assertEqual(run["metrics"]["loadedChunks"], 4_225)
            self.assertEqual(run["metrics"]["pendingMeshJobs"], 0)
            self.assertGreater(run["maxFrameGapMs"], 0)
            self.assertGreater(run["frameGapSamples"], 0)
            self.assertTrue(run["longTasks"])
            self.assertTrue(
                all(entry["durationMs"] >= 50 for entry in run["longTasks"])
            )
            self.assertLessEqual(
                max(entry["durationMs"] for entry in run["longTasks"]),
                run["maxFrameGapMs"] + 100,
            )
            self.assertLessEqual(
                max(ring["maxBatchMs"] for ring in run["rings"]), 1_000
            )

    def test_merged_chunk_borders_add_one_draw_call(self):
        overlays = self.metrics["overlays"]
        self.assertIsNone(overlays["failed"])
        self.assertEqual(overlays["radius"], 8)
        self.assertEqual(
            overlays["withBorders"]["drawCalls"],
            overlays["withoutBorders"]["drawCalls"] + 1,
        )
        self.assertEqual(
            overlays["withBorders"]["loadedChunks"],
            overlays["withoutBorders"]["loadedChunks"],
        )

    def test_post_gc_heap_is_stable_across_load_unload_cycles(self):
        leak = self.metrics["leak"]
        self.assertTrue(leak["enabled"])
        self.assertIsNone(leak["failed"])
        self.assertIs(leak["gcAvailable"], True)
        self.assertEqual(leak["chunks"], 289)
        self.assertEqual(len(leak["cycles"]), 5)
        heaps = [cycle["heapUsedBytes"] for cycle in leak["cycles"]]
        self.assertLessEqual(max(heaps) - min(heaps), LEAK_TOLERANCE_BYTES)
        self.assertLessEqual(
            abs(heaps[-1] - leak["baseline"]["heapUsedBytes"]),
            LEAK_TOLERANCE_BYTES,
        )
        for cycle in leak["cycles"]:
            self.assertEqual(cycle["geometries"], 0)
            self.assertEqual(cycle["meshCount"], 0)
            self.assertEqual(cycle["pendingMeshJobs"], 0)
            self.assertIsNotNone(cycle["memory"])
            self.assertGreater(cycle["memory"]["jsHeapTotalBytes"], 0)

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
