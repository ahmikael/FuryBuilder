"""Check the beginner/advanced user guides against the current UI and repository."""

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
GUIDES = [
    ROOT / "README.md",
    DOCS / "README.md",
    DOCS / "getting-started.md",
    DOCS / "advanced-workflow.md",
]
LINK_PATTERN = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class UserDocumentationTests(unittest.TestCase):
    def test_guides_exist_and_have_expected_structure(self):
        expected = {
            DOCS / "getting-started.md": ["# Getting started", "## 1. What you need", "## 13. Next steps"],
            DOCS / "advanced-workflow.md": [
                "# Advanced workflows",
                "## 1. How a write actually happens",
                "## 15. Where to read more",
            ],
            DOCS / "README.md": ["# FuryBuilder documentation", "## User guides"],
        }
        for path, headings in expected.items():
            self.assertTrue(path.is_file(), f"missing guide: {path}")
            text = read(path)
            for heading in headings:
                self.assertIn(heading, text, f"{path.name}: missing {heading}")

    def test_documented_project_modes_match_the_creation_dialog(self):
        source = read(ROOT / "apps" / "desktop" / "src" / "project" / "types.ts")
        modes = re.findall(r'mode:\s*"[^"]+",\s*label:\s*"([^"]+)"', source)
        self.assertGreaterEqual(len(modes), 4, "could not read the creation modes")
        guide = read(DOCS / "getting-started.md")
        for label in modes:
            self.assertIn(label, guide, f"getting-started.md does not describe {label}")

    def test_documented_bottom_tabs_match_the_editor_layout(self):
        source = read(ROOT / "apps" / "desktop" / "src" / "editor" / "layout.ts")
        labels = re.findall(r'\{\s*id:\s*"[^"]+",\s*label:\s*"([^"]+)"\s*\}', source)
        self.assertGreaterEqual(len(labels), 10, "could not read the bottom-panel tabs")
        guide = read(DOCS / "getting-started.md")
        for label in labels:
            self.assertIn(f"`{label}`", guide, f"getting-started.md does not list tab {label}")

    def test_guides_state_the_important_boundaries(self):
        beginner = read(DOCS / "getting-started.md")
        advanced = read(DOCS / "advanced-workflow.md")

        for term in (
            "pnpm tauri:dev",
            "browser",
            "preview",
            "Ctrl+S",
            "Java & World Generation",
            "Minecraft assets",
            "recovery",
            "Export .nbt",
        ):
            self.assertIn(term, beginner, f"getting-started.md is missing {term}")

        for term in (
            "expectedRevision",
            "65,536",
            "1,000,000",
            "4,096",
            "WARP",
            "SwiftShader",
            "no_history",
            "VanillaRebuild",
            "Reference overlays",
            "MCP setup",
        ):
            self.assertIn(term, advanced, f"advanced-workflow.md is missing {term}")

    def test_relative_markdown_links_resolve(self):
        missing = []
        for source in GUIDES:
            text = read(source)
            for raw_target in LINK_PATTERN.findall(text):
                target = raw_target.strip()
                if target.startswith(("http://", "https://", "mailto:", "#")):
                    continue
                target = target.split("#", 1)[0]
                if not target:
                    continue
                resolved = (source.parent / target).resolve()
                if not resolved.exists():
                    missing.append(f"{source.relative_to(ROOT)} -> {raw_target}")
        self.assertEqual(missing, [], "broken relative links:\n" + "\n".join(missing))

    def test_readme_points_to_the_user_guides(self):
        readme = read(ROOT / "README.md")
        self.assertIn("docs/getting-started.md", readme)
        self.assertIn("docs/advanced-workflow.md", readme)
        self.assertIn("docs/README.md", readme)
        self.assertIn("Getting started", readme)
        self.assertIn("Advanced workflows", readme)


if __name__ == "__main__":
    unittest.main()
