"""Validate the maintained known-limitations list and its documentation links."""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
LIMITATIONS = DOCS / "known-limitations.md"
README = ROOT / "README.md"
DOCS_INDEX = DOCS / "README.md"

DETAIL_RE = re.compile(r"^### (KL-\d{2}) — (.+)$", re.MULTILINE)
SUMMARY_RE = re.compile(r"^\| (KL-\d{2}) \|", re.MULTILINE)
LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
STATUS_RE = re.compile(
    r"\*\*Status:\*\* (by design|not implemented|environment-specific|pending M11)"
)


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class KnownLimitationsTests(unittest.TestCase):
    def setUp(self):
        self.text = read(LIMITATIONS)
        self.details = DETAIL_RE.findall(self.text)
        self.summary = SUMMARY_RE.findall(self.text)

    def test_limitations_exist_and_are_uniquely_numbered(self):
        self.assertGreaterEqual(len(self.details), 35, "expected a maintained, detailed list")
        ids = [entry[0] for entry in self.details]
        self.assertEqual(len(ids), len(set(ids)), "duplicate limitation id")
        numbers = sorted(int(item.split("-")[1]) for item in ids)
        self.assertEqual(numbers, list(range(1, len(numbers) + 1)), "ids must be sequential")
        self.assertEqual(set(ids), set(self.summary), "summary table and details differ")

    def test_every_entry_has_status_example_workaround_and_evidence(self):
        blocks = re.split(r"(?=^### KL-\d{2} — )", self.text, flags=re.MULTILINE)
        blocks = [block for block in blocks if block.startswith("### KL-")]
        self.assertEqual(len(blocks), len(self.details))
        for block in blocks:
            limitation_id = block.split(" ", 2)[1]
            self.assertRegex(block, STATUS_RE, f"{limitation_id}: missing/unknown status")
            for field in ("**Example:**", "**Workaround:**", "**Evidence:**"):
                self.assertIn(field, block, f"{limitation_id}: missing {field}")
            for field in ("**Status:**", "**Example:**", "**Workaround:**", "**Evidence:**"):
                value = block.split(field, 1)[1].split("**", 1)[0].strip()
                self.assertTrue(value, f"{limitation_id}: empty {field}")

    def test_required_boundaries_are_stated(self):
        for term in (
            "1.21.4",
            "DV 4189",
            "block entities",
            "VanillaRebuild",
            "world-folder export",
            "30 seconds",
            "permission_denied",
            "radius 32",
            "WARP",
            "SwiftShader",
            "Reference overlays are not enabled",
            "OpenCode CLI 2.3.2",
            "testing/test-matrix.md",
        ):
            self.assertIn(term, self.text, f"known-limitations.md is missing {term}")

    def test_readme_and_docs_index_link_to_the_list(self):
        readme = read(README)
        index = read(DOCS_INDEX)
        self.assertIn("docs/known-limitations.md", readme)
        self.assertIn("Known limitations", readme)
        self.assertIn("known-limitations.md", index)
        self.assertIn("Known limitations", index)

    def test_relative_markdown_links_resolve(self):
        missing = []
        for source in (README, DOCS_INDEX, LIMITATIONS):
            for raw_target in LINK_RE.findall(read(source)):
                target = raw_target.strip()
                if target.startswith(("http://", "https://", "mailto:", "#")):
                    continue
                target = target.split("#", 1)[0]
                if not target:
                    continue
                if not (source.parent / target).resolve().exists():
                    missing.append(f"{source.relative_to(ROOT)} -> {raw_target}")
        self.assertEqual(missing, [], "broken relative links:\n" + "\n".join(missing))


if __name__ == "__main__":
    unittest.main()
