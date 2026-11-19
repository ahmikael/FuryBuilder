"""Check the M10.6 diagnostics evidence from the real Tauri WebView2 app."""

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
    / "2026-11-19-diagnostics-recovery"
)


class DiagnosticsEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.report = json.loads((EVIDENCE / "diagnostics.json").read_text(encoding="utf-8"))

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

    def test_log_is_written_redacted_and_cleared(self):
        log = self.report["log"]
        self.assertGreater(log["beforeBytes"], 0)
        self.assertGreaterEqual(log["afterBytes"], log["beforeBytes"])
        self.assertTrue(log["containsUiError"])
        self.assertTrue(log["containsPathPlaceholder"])
        self.assertFalse(log["containsPrivatePath"])
        self.assertEqual(log["afterClearBytes"], 0)
        self.assertEqual(log["afterClearFiles"], 0)

    def test_settings_state_is_explicit(self):
        state = self.report["state"]
        self.assertEqual(state["dialogTitle"], "Settings")
        self.assertTrue(state["logPathShown"])
        self.assertTrue(state["appVersionShown"])
        self.assertTrue(state["rotationShown"])
        self.assertTrue(state["afterClear"]["clearNotice"])
        self.assertTrue(state["afterClear"]["emptyState"])

    def test_screenshot_is_a_valid_png_with_matching_hash(self):
        screenshot = self.report["screenshot"]
        raw = (EVIDENCE / screenshot["file"]).read_bytes()
        self.assertEqual(raw[:8], b"\x89PNG\r\n\x1a\n")
        width, height = struct.unpack(">II", raw[16:24])
        self.assertEqual((width, height), (screenshot["width"], screenshot["height"]))
        self.assertEqual(len(raw), screenshot["bytes"])
        self.assertEqual(hashlib.sha256(raw).hexdigest(), screenshot["sha256"])
        self.assertGreater(width, 1_000)
        self.assertGreater(height, 700)


if __name__ == "__main__":
    unittest.main()
