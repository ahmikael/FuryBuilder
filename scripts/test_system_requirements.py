"""Validate the M11.6 system requirements and beta limitations documentation."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REQUIREMENTS = ROOT / "docs" / "system-requirements.md"
LIMITATIONS = ROOT / "docs" / "known-limitations.md"
INSTALL = ROOT / "docs" / "install-windows.md"
GETTING_STARTED = ROOT / "docs" / "getting-started.md"
README = ROOT / "README.md"
DOCS_INDEX = ROOT / "docs" / "README.md"
CI = ROOT / ".github" / "workflows" / "ci.yml"
LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class SystemRequirementsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = read(REQUIREMENTS)

    def row(self, label: str) -> str:
        rows = [line for line in self.text.splitlines() if label in line]
        self.assertTrue(rows, f"missing table row: {label}")
        return rows[0]

    def test_page_states_the_beta_scope(self):
        self.assertIn("# System requirements (Windows beta)", self.text)
        self.assertIn("Windows beta 0.1.0", self.text)
        self.assertIn("Minecraft Java **1.21.4 / DataVersion 4189**", self.text)
        self.assertIn("Last reviewed: 2026-11-19", self.text)

    def test_supported_baseline_is_explicit(self):
        for term in (
            "64-bit Windows 10 or Windows 11",
            "Microsoft Edge WebView2 Runtime",
            "WebGL 2.0",
            "Direct3D 11",
            "8 GB is the practical baseline target",
            "16 GB is the tested reference",
            "5.9 MB (NSIS)",
            "7.8 MB (MSI)",
            "Keep at least 1 GB free",
            "visible viewport",
            "Not required for `Empty Project`",
            "%APPDATA%\\com.furybuilder.desktop",
        ):
            self.assertIn(term, self.text, f"missing baseline statement: {term}")

    def test_tested_reference_values_match_recorded_evidence(self):
        for term in (
            "Windows 11 Pro",
            "10.0.26300",
            "Intel Core i5-9600KF",
            "16 GB",
            "NVIDIA GeForce GTX 1060 3 GB",
            "32.0.15.6094",
            "157.0.4341.60",
            "209d4f0c849ad952494cbd4a7947f880cb2c1ac61402bf848b504014eae7eea9",
            "5,879,723",
            "7,802,880",
            "ad21b7f37ed092fa1b7a0e82b3b30631deb62cdda6f392037d85ed1e02ec209c",
            "52622c9b503e7a3b0156d3649c1c87b25fc85f58558e6d4c4caf06e0fadc618e",
            "21.0.12.1+1",
            "4707d00eb834b446575d89a61a11b5d548d8c001",
            "OpenCode CLI `2.3.2`",
        ):
            self.assertIn(term, self.text, f"missing tested-reference value: {term}")

        installer_readme = read(
            ROOT
            / "docs"
            / "testing"
            / "evidence"
            / "2026-11-19-windows-installer"
            / "README.md"
        )
        for term in ("5,879,723", "7,802,880", "ad21b7f3", "52622c9b"):
            self.assertIn(term, installer_readme, f"installer evidence lacks {term}")

    def test_feature_dependencies_match_the_supported_modes(self):
        empty = self.row("`Empty Project` / `Flat base`")
        self.assertEqual(empty.count("| No"), 5, empty)

        vanilla = self.row("`Vanilla World` generation")
        self.assertIn("| No", vanilla, "Vanilla World must not require a client install")
        self.assertIn("| Yes", vanilla, "Vanilla World needs Java and the server JAR")
        self.assertIn("Only for optional downloads", vanilla, vanilla)

        mcp = self.row("MCP bridge")
        self.assertIn("| Yes", mcp, "MCP needs the Node.js bridge")
        self.assertIn("local named pipe", mcp, mcp)

        self.assertIn("never accepts it automatically", self.text)
        self.assertIn("never bundles Java or the server JAR", self.text)

    def test_unvalidated_configurations_are_not_hidden(self):
        for term in (
            "Windows 10 is not tested",
            "8 GB has **not** been validated",
            "ARM64 are not tested",
            "integrated GPUs, switching and other vendors are not tested",
            "only one physical display adapter",
            "Windows 10, Windows on ARM, Windows Server",
            "Code-signed installers, automatic updates",
            "External beta-user field data",
            "KL-43",
        ):
            self.assertIn(term, self.text, f"missing unvalidated statement: {term}")

    def test_user_facing_docs_link_to_the_requirements_page(self):
        self.assertIn("docs/system-requirements.md", read(README))
        self.assertIn("system-requirements.md", read(DOCS_INDEX))
        self.assertIn("system-requirements.md", read(GETTING_STARTED))
        self.assertIn("system-requirements.md", read(INSTALL))

    def test_relative_links_resolve(self):
        missing = []
        for source in (
            README,
            DOCS_INDEX,
            GETTING_STARTED,
            INSTALL,
            REQUIREMENTS,
            LIMITATIONS,
        ):
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

    def test_known_limitations_include_the_beta_evidence_gap(self):
        limitations = read(LIMITATIONS)
        self.assertIn("Windows beta 0.1.0", limitations)
        self.assertIn("system-requirements.md", limitations)
        self.assertIn("| KL-43 |", limitations)
        block = limitations.split("### KL-43", 1)[1].split("###", 1)[0]
        self.assertIn("**Status:** pending M11", block)
        self.assertIn("No external user", block)
        self.assertIn("system-requirements.md", block)
        self.assertIn("java-server-interop.md", block)

    def test_ci_runs_the_documentation_validators(self):
        ci = read(CI)
        self.assertIn("python scripts/test_system_requirements.py", ci)
        self.assertIn("python scripts/test_known_limitations.py", ci)

    def test_no_absolute_paths_or_secrets_are_published(self):
        combined = "\n".join(
            read(path)
            for path in (
                REQUIREMENTS,
                LIMITATIONS,
                README,
                DOCS_INDEX,
                GETTING_STARTED,
                INSTALL,
            )
        )
        self.assertNotIn("C:\\Users\\", combined)
        self.assertNotIn("D:\\mitrium", combined)
        self.assertNotIn("\\\\pipe", combined)
        self.assertNotIn("token=", combined.lower())
        self.assertNotIn("bearer ", combined.lower())
        self.assertNotIn("-----BEGIN", combined)


if __name__ == "__main__":
    unittest.main()
