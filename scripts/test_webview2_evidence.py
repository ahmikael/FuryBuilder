"""Check the integrity and recorded results of the WebView2 hardware run."""

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
    / "2026-11-18-webview2"
)
PNG_SHA256 = "4782eb2130cb7c74347045dedf8c8ce42cf5ed946b9285b3b09a9020bc4023f1"


class WebViewEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.metrics = json.loads((EVIDENCE / "metrics.json").read_text(encoding="utf-8"))

    def test_recorded_gpu_scene_and_capture_are_complete(self):
        self.assertTrue(self.metrics["webglVersion"].startswith("WebGL 2.0"))
        self.assertIn("NVIDIA GeForce GTX 1060", self.metrics["gpuRenderer"])
        self.assertEqual(self.metrics["glError"], 0)
        scene = self.metrics["scene"]
        self.assertEqual(scene["chunks"], 289)
        self.assertEqual(scene["nonAirBlocks"], 616_752)
        self.assertEqual(scene["metrics"]["loadedChunks"], 289)
        self.assertGreater(scene["metrics"]["triangles"], 0)
        capture = self.metrics["capture"]
        self.assertEqual(capture["worldRevision"], 1)
        self.assertEqual(capture["loadedChunks"], 289)
        self.assertEqual(capture["missingChunks"], 0)
        self.assertIs(capture["partial"], False)
        self.assertGreaterEqual(capture["sampledColors"], 16)

    def test_png_matches_hash_length_and_recorded_resolution(self):
        image = (EVIDENCE / "viewport.png").read_bytes()
        self.assertGreaterEqual(len(image), 24)
        self.assertEqual(image[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(image[12:16], b"IHDR")
        self.assertEqual(list(struct.unpack_from(">II", image, 16)), [1600, 900])
        capture = self.metrics["capture"]
        self.assertEqual(capture["resolution"], [1600, 900])
        self.assertEqual(capture["pngBytes"], len(image))
        self.assertEqual(capture["pngSha256"], PNG_SHA256)
        self.assertEqual(hashlib.sha256(image).hexdigest(), PNG_SHA256)


if __name__ == "__main__":
    unittest.main()
