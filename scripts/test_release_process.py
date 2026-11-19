"""Validate the M11 versioning, changelog, migration and backup policy."""

import json
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
VERSION_FILES = [
    "package.json",
    "apps/desktop/package.json",
    "mcp/bridge/package.json",
    "apps/desktop/src-tauri/tauri.conf.json",
]


def run_node(*args, cwd=ROOT):
    return subprocess.run(
        ["node", str(ROOT / "scripts" / "version.mjs"), *args],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )


class ReleaseProcessTests(unittest.TestCase):
    def test_version_json_is_the_source_of_truth(self):
        version = json.loads((ROOT / "version.json").read_text(encoding="utf-8"))["version"]
        self.assertRegex(version, SEMVER)
        for relative in VERSION_FILES:
            data = json.loads((ROOT / relative).read_text(encoding="utf-8"))
            self.assertEqual(data["version"], version, relative)
        cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        section = re.search(r"\[workspace\.package\](.*?)(?=\n\[|\Z)", cargo, re.S)
        self.assertIsNotNone(section)
        self.assertIn(f'version = "{version}"', section.group(1))
        bridge = (ROOT / "mcp" / "bridge" / "src" / "capabilities.ts").read_text(encoding="utf-8")
        self.assertIn(f'export const BRIDGE_VERSION = "{version}"', bridge)
        apps = (ROOT / "mcp" / "bridge" / "src" / "apps.ts").read_text(encoding="utf-8")
        self.assertIn(f'export const OBSERVATION_APP_VERSION = "{version}"', apps)

    def test_version_check_passes_on_the_repository(self):
        result = run_node("check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("matches 8 manifests", result.stdout)

    def test_version_check_detects_drift_and_set_repairs_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = {
                "version.json": '{\n  "version": "0.1.0"\n}\n',
                "package.json": '{\n  "version": "0.1.0"\n}\n',
                "apps/desktop/package.json": '{\n  "version": "0.1.0"\n}\n',
                "mcp/bridge/package.json": '{\n  "version": "0.1.0"\n}\n',
                "apps/desktop/src-tauri/tauri.conf.json": '{\n  "version": "0.1.0"\n}\n',
                "Cargo.toml": '[workspace.package]\nversion = "0.1.0"\n',
                "mcp/bridge/src/capabilities.ts": 'export const BRIDGE_VERSION = "0.1.0";\n',
                "mcp/bridge/src/apps.ts": 'export const OBSERVATION_APP_VERSION = "0.1.0";\n',
            }
            for relative, content in files.items():
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(content, encoding="utf-8")

            self.assertEqual(run_node("check", "--root", str(root)).returncode, 0)
            (root / "package.json").write_text('{\n  "version": "9.9.9"\n}\n', encoding="utf-8")
            drift = run_node("check", "--root", str(root))
            self.assertEqual(drift.returncode, 1)
            self.assertIn("MISMATCH", drift.stdout + drift.stderr)

            updated = run_node("set", "1.2.3", "--root", str(root))
            self.assertEqual(updated.returncode, 0, updated.stderr)
            self.assertEqual(run_node("check", "--root", str(root)).returncode, 0)
            self.assertEqual(
                json.loads((root / "version.json").read_text(encoding="utf-8"))["version"],
                "1.2.3",
            )
            self.assertIn(
                'version = "1.2.3"', (root / "Cargo.toml").read_text(encoding="utf-8")
            )
            self.assertIn(
                'BRIDGE_VERSION = "1.2.3"',
                (root / "mcp/bridge/src/capabilities.ts").read_text(encoding="utf-8"),
            )

    def test_changelog_is_maintained_and_current(self):
        changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
        version = json.loads((ROOT / "version.json").read_text(encoding="utf-8"))["version"]
        self.assertTrue(changelog.startswith("# Changelog"))
        self.assertIn("## [Unreleased]", changelog)
        self.assertIn(f"## [{version}]", changelog)
        for section in ("### Added", "### Changed", "### Fixed", "### Security"):
            self.assertIn(section, changelog)
        self.assertIn("NSIS", changelog)
        self.assertIn("version:check", changelog)

    def test_release_docs_cover_versioning_and_backup_rules(self):
        versioning = (ROOT / "docs" / "release" / "versioning.md").read_text(encoding="utf-8")
        for term in (
            "version.json",
            "pnpm version:check",
            "PROJECT_SCHEMA_VERSION",
            "SNAPSHOT_FORMAT_VERSION",
            "allowDowngrades",
            "ed1f6f4c-a0f3-52c1-8d7a-18326f6a292c",
            "CHANGELOG.md",
        ):
            self.assertIn(term, versioning)

        backup = (ROOT / "docs" / "release" / "backup-policy.md").read_text(encoding="utf-8")
        for term in (
            "project.json.v<old>.bak",
            "Newest 8",
            "Newest 5",
            "%APPDATA%",
            "byte for byte",
            "unsupported_version",
        ):
            self.assertIn(term, backup)

        index = (ROOT / "docs" / "release" / "README.md").read_text(encoding="utf-8")
        self.assertIn("versioning.md", index)
        self.assertIn("backup-policy.md", index)

    def test_ci_and_package_scripts_enforce_the_version_check(self):
        package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
        self.assertEqual(package["scripts"]["version:check"], "node scripts/version.mjs check")
        self.assertEqual(package["scripts"]["version:set"], "node scripts/version.mjs set")
        ci = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("pnpm version:check", ci)

    def test_project_store_retention_matches_the_documented_policy(self):
        store = (ROOT / "crates" / "project-store" / "src" / "store.rs").read_text(encoding="utf-8")
        self.assertIn("pub const MAX_SNAPSHOTS: usize = 8;", store)
        self.assertIn("pub const MAX_CHECKPOINTS: usize = 5;", store)
        self.assertIn("pub const MAX_MANIFEST_BACKUPS: usize = 5;", store)
        self.assertIn('strip_prefix("project.json.bak-")', store)
        migration = store[store.index("if version < PROJECT_SCHEMA_VERSION") :]
        self.assertIn("fs::copy(&path, &backup)", migration)

        test_source = (ROOT / "crates" / "project-store" / "tests" / "migrations.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("older_schema_is_migrated_with_a_verbatim_backup", test_source)
        self.assertIn("project.json.v0.bak", test_source)
        self.assertIn("byte-identical", test_source)
        store_tests = (ROOT / "crates" / "project-store" / "src" / "store.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn("per_save_manifest_backups_are_bounded", store_tests)

    def test_no_unrelated_repository_state_is_required(self):
        # Guard against accidentally pointing the test at a copied scratch tree.
        self.assertTrue((ROOT / "FURYBUILDER_MASTER_PLAN.md").is_file())
        self.assertTrue((ROOT / "scripts" / "version.mjs").is_file())
        self.assertTrue(shutil.which("node"), "node is required for the version tooling")


if __name__ == "__main__":
    unittest.main()
