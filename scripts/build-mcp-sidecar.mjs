#!/usr/bin/env node
/**
 * Stages the Rust MCP sidecar where the Tauri bundler expects it.
 *
 * Tauri's `externalBin` entries are resolved as `<path>-<target-triple><exe>`. The
 * sidecar must be built for the same target as the desktop app, so this script asks
 * rustc for the host triple, builds the crate for that target and copies the artifact
 * to `apps/desktop/src-tauri/binaries/`.
 *
 * Usage:
 *   node scripts/build-mcp-sidecar.mjs            # release build for the installer
 *   node scripts/build-mcp-sidecar.mjs --debug    # faster local validation
 */

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

const ROOT = process.cwd();
const DEBUG = process.argv.includes("--debug");
const PACKAGE = "furybuilder-mcp-bridge";
const BINARY = "furybuilder-mcp";

function rustHostTriple() {
  const output = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const match = /^host:\s*(\S+)$/m.exec(output);
  if (!match) {
    throw new Error("rustc -vV did not report a host triple");
  }
  return match[1];
}

function cargoMetadata() {
  const output = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    cwd: ROOT,
    encoding: "utf8",
  });
  return JSON.parse(output);
}

function main() {
  const triple = rustHostTriple();
  const metadata = cargoMetadata();
  const workspaceRoot = metadata.workspace_root;
  const targetRoot = metadata.target_directory;
  const profile = DEBUG ? "debug" : "release";
  const args = ["build", "--package", PACKAGE, "--target", triple];
  if (!DEBUG) {
    args.push("--release");
  }
  console.log(`Building ${BINARY} (${profile}) for ${triple}...`);
  execFileSync("cargo", args, { cwd: workspaceRoot, stdio: "inherit" });

  const extension = triple.includes("windows") ? ".exe" : "";
  const source = resolve(targetRoot, triple, profile, `${BINARY}${extension}`);
  const destinationDirectory = resolve(workspaceRoot, "apps", "desktop", "src-tauri", "binaries");
  const destination = resolve(destinationDirectory, `${BINARY}-${triple}${extension}`);
  mkdirSync(destinationDirectory, { recursive: true });
  copyFileSync(source, destination);
  console.log(`Staged ${destination}`);
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
