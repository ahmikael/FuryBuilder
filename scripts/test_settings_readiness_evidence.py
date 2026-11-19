"""Check the M10.5 dependency-settings evidence from the real Tauri WebView2 app."""

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
    / "2026-11-19-settings-readiness"
)


class SettingsReadinessEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.report = json.loads(
            (EVIDENCE / "settings-readiness.json").read_text(encoding="utf-8")
        )

    def test_records_an_isolated_real_app_run(self):
        self.assertEqual(self.report["schema"], 1)
        self.assertTrue(self.report["isolatedAppData"])
        self.assertEqual(self.report["origin"], "http://127.0.0.1:1420")
        self.assertEqual(
            self.report["app"]["executable"],
            "target/debug/furybuilder-desktop.exe",
        )
        self.assertEqual(len(self.report["app"]["sha256"]), 64)
        self.assertEqual(self.report["consoleErrors"], [])

    def test_all_ui_checks_passed(self):
        checks = self.report["checks"]
        self.assertTrue(checks)
        for name, value in checks.items():
            with self.subTest(check=name):
                self.assertIs(value, True)

    def test_reports_the_expected_blocked_state(self):
        state = self.report["state"]
        self.assertEqual(state["dialogTitle"], "Settings")
        self.assertTrue(state["groupHeading"])
        self.assertTrue(state["optionalLabel"])
        self.assertEqual(state["readinessState"], "blocked")
        self.assertIn("Setup required", state["readinessText"])
        self.assertIn("3 blocker(s)", state["readinessText"])
        self.assertTrue(state["assetEmptyState"])
        self.assertTrue(state["assetCacheNotice"])
        self.assertTrue(state["manualHints"])
        self.assertTrue(state["offlineFallback"])

    def test_screenshot_is_a_valid_png_with_matching_hash(self):
        screenshot = self.report["screenshot"]
        raw = (EVIDENCE / screenshot["file"]).read_bytes()
        self.assertEqual(raw[:8], b"\x89PNG\r\n\x1a\n")
        width, height = struct.unpack(">II", raw[16:24])
        self.assertEqual((width, height), (screenshot["width"], screenshot["height"]))
        self.assertEqual(len(raw), screenshot["bytes"])
        self.assertEqual(
            hashlib.sha256(raw).hexdigest(),
            screenshot["sha256"],
        )
        self.assertGreater(width, 1_000)
        self.assertGreater(height, 700)


if __name__ == "__main__":
    unittest.main()
