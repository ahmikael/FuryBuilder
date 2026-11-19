"""Check integrity and recorded results of the M10.1 renderer profile run."""

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
    / "2026-11-19-webview2-profile"
)


class WebViewProfileEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.metrics = json.loads((EVIDENCE / "metrics.json").read_text(encoding="utf-8"))
        self.summary = json.loads(
            (EVIDENCE / "cpu-profile-summary.json").read_text(encoding="utf-8")
        )

    def test_profile_records_real_renderer_work(self):
        self.assertEqual(self.metrics["kind"], "webview2-large-map-stress")
        self.assertEqual(self.metrics["schemaVersion"], 1)
        self.assertEqual(
            self.metrics["profiling"],
            {
                "enabled": True,
                "samplingIntervalMicros": 250,
                "summaryFile": "cpu-profile-summary.json",
                "note": "The profiled run has profiling overhead; use it for relative hot spots, not for the limit table.",
            },
        )
        self.assertGreater(self.summary["sampleCount"], 10_000)
        self.assertGreater(self.summary["durationMs"], 1_000)
        self.assertGreaterEqual(self.summary["idleShare"], 0)
        self.assertLessEqual(self.summary["idleShare"], 1)
        self.assertGreaterEqual(self.summary["gcShare"], 0)
        self.assertLessEqual(self.summary["gcShare"], 1)
        names = {row["functionName"] for row in self.summary["topSelfTime"]}
        self.assertTrue(
            {
                "cachedBoxShapeFor",
                "buildChunkMesh",
                "pushQuad",
                "readPackedIndex",
                "neighborName",
            }.issubset(names)
        )
        self.assertTrue(
            any(row["url"].endswith("/src/renderer/mesher.ts") for row in self.summary["topSelfTime"])
        )

    def test_profile_covers_snapshots_and_streaming(self):
        snapshots = self.metrics["snapshots"]
        self.assertEqual([snapshot["radius"] for snapshot in snapshots], [8, 16, 24])
        for snapshot in snapshots:
            self.assertIsNone(snapshot["failed"])
            self.assertEqual(snapshot["metrics"]["loadedChunks"], snapshot["chunks"])
            self.assertEqual(snapshot["capture"]["missingChunks"], 0)
            self.assertIs(snapshot["capture"]["partial"], False)
        streaming = self.metrics["streaming"]
        self.assertIsNone(streaming["failed"])
        self.assertEqual(
            [run["batchChunks"] for run in streaming["runs"]], [16, 32, 64]
        )
        for run in streaming["runs"]:
            self.assertIsNone(run["failed"])
            self.assertEqual(run["metrics"]["loadedChunks"], 2_401)
            self.assertGreater(run["totalMs"], 0)

    def test_gpu_ram_and_png_are_recorded(self):
        environment = self.metrics["environment"]
        self.assertTrue(environment["webglVersion"].startswith("WebGL 2.0"))
        self.assertEqual(environment["glError"], 0)
        self.assertIs(environment["gpuTimerAvailable"], True)
        self.assertGreater(self.metrics["rendererInfo"]["calls"], 0)
        self.assertGreater(self.metrics["rendererInfo"]["triangles"], 0)
        memory = self.metrics["memory"]
        self.assertGreater(memory["jsHeapUsedBytes"], 0)
        self.assertGreater(memory["jsHeapTotalBytes"], 0)
        self.assertGreaterEqual(memory["jsHeapTotalBytes"], memory["jsHeapUsedBytes"])

        image = (EVIDENCE / "viewport.png").read_bytes()
        self.assertEqual(image[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(image[12:16], b"IHDR")
        self.assertEqual(list(struct.unpack_from(">II", image, 16)), [1600, 900])
        capture = self.metrics["capture"]
        self.assertEqual(capture["pngBytes"], len(image))
        self.assertEqual(capture["pngSha256"], hashlib.sha256(image).hexdigest())


if __name__ == "__main__":
    unittest.main()
