#!/usr/bin/env node
/**
 * Single-source application version for FuryBuilder.
 *
 * `version.json` is authoritative. Every other manifest that the installer, Cargo
 * workspace or pnpm workspace reads must agree with it. Release builds fail fast when
 * one file drifts instead of shipping an installer whose registry version, executable
 * resource version and about screen disagree.
 */

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const SOURCE_FILE = "version.json";
const JSON_TARGETS = [
  "package.json",
  "apps/desktop/package.json",
  "apps/desktop/src-tauri/tauri.conf.json",
];

function readText(root, relative) {
  return readFileSync(resolve(root, relative), "utf8");
}

function writeText(root, relative, text) {
  writeFileSync(resolve(root, relative), text, "utf8");
}

function assertSemver(version, source) {
  if (!SEMVER.test(version)) {
    throw new Error(
      `${source}: expected MAJOR.MINOR.PATCH, found ${JSON.stringify(version)}`,
    );
  }
}

function readSourceVersion(root) {
  let parsed;
  try {
    parsed = JSON.parse(readText(root, SOURCE_FILE));
  } catch (error) {
    throw new Error(`cannot read ${SOURCE_FILE}: ${error.message}`);
  }
  if (typeof parsed.version !== "string") {
    throw new Error(`${SOURCE_FILE} must contain a string "version"`);
  }
  assertSemver(parsed.version, SOURCE_FILE);
  return parsed.version;
}

function readJsonVersion(root, relative) {
  let parsed;
  try {
    parsed = JSON.parse(readText(root, relative));
  } catch (error) {
    throw new Error(`cannot parse ${relative}: ${error.message}`);
  }
  if (typeof parsed.version !== "string") {
    throw new Error(`${relative} has no string "version" field`);
  }
  return parsed.version;
}

function readCargoVersion(root) {
  const text = readText(root, "Cargo.toml");
  const start = text.indexOf("[workspace.package]");
  if (start < 0) {
    throw new Error("Cargo.toml has no [workspace.package] section");
  }
  const tail = text.slice(start);
  const end = tail.search(/\r?\n\[/);
  const section = end < 0 ? tail : tail.slice(0, end);
  const match = /(?:^|\r?\n)[ \t]*version[ \t]*=[ \t]*"([^"]+)"/.exec(section);
  if (!match) {
    throw new Error("Cargo.toml [workspace.package] has no version");
  }
  return match[1];
}

function setJsonVersion(root, relative, version) {
  const text = readText(root, relative);
  const pattern = /"version"\s*:\s*"[^"]*"/;
  if (!pattern.test(text)) {
    throw new Error(`${relative} has no "version" field`);
  }
  writeText(root, relative, text.replace(pattern, `"version": "${version}"`));
}

function setCargoVersion(root, version) {
  const text = readText(root, "Cargo.toml");
  const start = text.indexOf("[workspace.package]");
  if (start < 0) {
    throw new Error("Cargo.toml has no [workspace.package] section");
  }
  const tail = text.slice(start);
  const end = tail.search(/\r?\n\[/);
  const section = end < 0 ? tail : tail.slice(0, end);
  const pattern = /((?:^|\r?\n)[ \t]*version[ \t]*=[ \t]*")[^"]+(")/;
  if (!pattern.test(section)) {
    throw new Error("Cargo.toml [workspace.package] has no version");
  }
  const updated = section.replace(pattern, `$1${version}$2`);
  writeText(
    root,
    "Cargo.toml",
    text.slice(0, start) + updated + tail.slice(section.length),
  );
}

function collect(root) {
  const version = readSourceVersion(root);
  const findings = [
    { file: SOURCE_FILE, version },
    ...JSON_TARGETS.map((file) => ({
      file,
      version: readJsonVersion(root, file),
    })),
    { file: "Cargo.toml", version: readCargoVersion(root) },
  ];
  return { version, findings };
}

function check(root) {
  const { version, findings } = collect(root);
  const mismatches = findings.filter((item) => item.version !== version);
  if (mismatches.length > 0) {
    console.error(`Version mismatch: ${SOURCE_FILE} is ${version}`);
    for (const item of findings) {
      const marker = item.version === version ? "ok" : "MISMATCH";
      console.error(`  ${marker.padEnd(8)} ${item.file}: ${item.version}`);
    }
    process.exitCode = 1;
    return false;
  }
  console.log(`Version ${version} matches ${findings.length} manifests.`);
  return true;
}

function setVersion(root, version) {
  assertSemver(version, "new version");
  readSourceVersion(root);
  for (const file of JSON_TARGETS) {
    setJsonVersion(root, file, version);
  }
  setCargoVersion(root, version);
  writeText(root, SOURCE_FILE, `${JSON.stringify({ version }, null, 2)}\n`);
  console.log(`Updated ${JSON_TARGETS.length + 2} manifests to ${version}.`);
  console.log(
    "Cargo.lock will be refreshed by the next cargo build; commit it with the bump.",
  );
}

function parseArguments(argv) {
  const args = [...argv];
  let root = process.cwd();
  const rootIndex = args.indexOf("--root");
  if (rootIndex >= 0) {
    const value = args[rootIndex + 1];
    if (!value) {
      throw new Error("--root requires a directory");
    }
    root = resolve(value);
    args.splice(rootIndex, 2);
  }
  return { root, command: args[0], value: args[1] };
}

function main(argv) {
  const { root, command, value } = parseArguments(argv);
  if (command === "check") {
    check(root);
    return;
  }
  if (command === "set") {
    if (!value) {
      throw new Error(
        "usage: node scripts/version.mjs set <MAJOR.MINOR.PATCH>",
      );
    }
    setVersion(root, value);
    return;
  }
  throw new Error(
    "usage: node scripts/version.mjs <check|set> [version] [--root <dir>]",
  );
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

export { check, collect, readCargoVersion, setVersion };
