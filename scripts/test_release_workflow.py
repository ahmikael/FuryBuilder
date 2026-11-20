"""Validate the tag-driven Windows release workflow and the Tauri updater wiring."""

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
CONFIG = ROOT / "apps" / "desktop" / "src-tauri" / "tauri.conf.json"
CAPABILITY = ROOT / "apps" / "desktop" / "src-tauri" / "capabilities" / "default.json"
PACKAGE = ROOT / "package.json"
RELEASE_TAG_PATTERN = re.compile(r"v\*\.\*\.\*")


class ReleaseWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.ci_workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        cls.config = json.loads(CONFIG.read_text(encoding="utf-8"))
        cls.capability = json.loads(CAPABILITY.read_text(encoding="utf-8"))
        cls.package = json.loads(PACKAGE.read_text(encoding="utf-8"))
        cls.versioning = (ROOT / "docs" / "release" / "versioning.md").read_text(
            encoding="utf-8"
        )
        cls.install = (ROOT / "docs" / "install-windows.md").read_text(encoding="utf-8")
        cls.limitations = (ROOT / "docs" / "known-limitations.md").read_text(encoding="utf-8")

    def test_release_runs_on_version_tags_only(self):
        self.assertIn("name: Release", self.workflow)
        self.assertIn('tags:\n      - "v*.*.*"', self.workflow)
        self.assertIsNotNone(RELEASE_TAG_PATTERN.search(self.workflow))
        self.assertIn("permissions:\n  contents: write", self.workflow)
        self.assertIn("workflow_dispatch:", self.workflow)

    def test_release_validates_tag_and_runs_existing_checks(self):
        self.assertIn("version.json", self.workflow)
        self.assertIn("pnpm version:check", self.workflow)
        self.assertIn("pnpm test:release-notes", self.workflow)
        self.assertIn("pnpm typecheck", self.workflow)
        self.assertIn("pnpm test", self.workflow)
        self.assertIn("cargo test --workspace", self.workflow)

    def test_release_uses_conventional_commit_notes(self):
        self.assertIn("scripts/release-notes.mjs", self.workflow)
        self.assertIn("--output release-notes.md", self.workflow)
        self.assertIn("gh release create", self.workflow)
        self.assertIn("gh release edit", self.workflow)
        self.assertIn("--draft=false", self.workflow)
        self.assertEqual(
            self.package["scripts"]["test:release-notes"],
            "node --test scripts/release-notes.test.mjs",
        )

    def test_release_builds_and_uploads_signed_windows_artifacts(self):
        self.assertIn("tauri-apps/tauri-action@v0", self.workflow)
        self.assertIn("TAURI_SIGNING_PRIVATE_KEY", self.workflow)
        self.assertIn("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", self.workflow)
        self.assertIn("--bundles nsis", self.workflow)
        self.assertIn("scripts/build-mcp-sidecar.mjs", self.workflow)
        self.assertIn("furybuilder-mcp-x86_64-pc-windows-msvc.exe", self.workflow)
        self.assertNotIn("--bundles nsis,msi", self.workflow)
        self.assertNotIn(".msi", self.workflow)
        self.assertIn("uploadUpdaterSignatures: true", self.workflow)
        self.assertIn("releaseBody: ${{ steps.notes.outputs.body }}", self.workflow)
        self.assertIn("latest.json", self.workflow)
        self.assertIn("FuryBuilder_${version}_x64-setup.exe", self.workflow)
        self.assertIn("FuryBuilder_${version}_x64-setup.exe.sig", self.workflow)
        self.assertIn("SHA-256", self.workflow)
        self.assertIn("--latest", self.workflow)

    def test_bundle_config_ships_only_the_nsis_installer(self):
        bundle = self.config["bundle"]
        self.assertEqual(bundle["targets"], ["nsis"])
        self.assertEqual(bundle["externalBin"], ["binaries/furybuilder-mcp"])
        self.assertNotIn("wix", bundle["windows"])
        self.assertEqual(bundle["windows"]["nsis"]["installMode"], "currentUser")

    def test_updater_is_configured_and_exposes_only_required_permissions(self):
        bundle = self.config["bundle"]
        self.assertIs(bundle["createUpdaterArtifacts"], True)
        updater = self.config["plugins"]["updater"]
        self.assertGreater(len(updater["pubkey"]), 80)
        self.assertTrue(updater["pubkey"].startswith("dW50cnVzdGVkIGNvbW1lbnQ6"))
        self.assertEqual(
            updater["endpoints"],
            ["https://github.com/ahmikael/FuryBuilder/releases/latest/download/latest.json"],
        )
        self.assertEqual(updater["windows"]["installMode"], "passive")

        permissions = self.capability["permissions"]
        self.assertIn("updater:default", permissions)
        self.assertIn("process:allow-restart", permissions)

    def test_release_notes_script_is_documented_and_current(self):
        notes = (ROOT / "scripts" / "release-notes.mjs").read_text(encoding="utf-8")
        for term in (
            "parseConventionalCommit",
            "Breaking Changes",
            "Build System",
            "Tests",
            "Continuous Integration",
            "Code Style",
            "Chores",
        ):
            self.assertIn(term, notes)
        for term in ("release-notes.mjs", "Conventional Commits", "latest.json"):
            self.assertIn(term, self.versioning)
        for term in ("auto-update", "updater", "GitHub Releases"):
            self.assertIn(term, self.install)
        self.assertNotIn("MSI", notes)
        self.assertNotIn("MSI", self.install)
        self.assertNotIn("MSI", self.limitations)
        self.assertNotIn("Tauri auto-updater is not configured", self.versioning)
        self.assertNotIn("no auto-update channel", self.limitations)

    def test_release_workflow_has_safe_rerun_and_cleanup_rules(self):
        self.assertIn("isDraft", self.workflow)
        self.assertIn("already published", self.workflow)
        self.assertIn("gh release delete", self.workflow)
        self.assertIn("--verify-tag", self.workflow)

    def test_sidecar_is_staged_before_rust_compilation(self):
        for workflow, sidecar in (
            (self.workflow, "scripts/build-mcp-sidecar.mjs"),
            (self.ci_workflow, "scripts/build-mcp-sidecar.mjs --debug"),
        ):
            staged = workflow.index(sidecar)
            for command in ("cargo fmt --all", "cargo clippy --workspace", "cargo test --workspace"):
                self.assertLess(
                    staged,
                    workflow.index(command),
                    f"{sidecar} must run before {command}; Tauri validates externalBin during compilation.",
                )


if __name__ == "__main__":
    unittest.main()
