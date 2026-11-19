#!/usr/bin/env node
/**
 * Builds the third-party dependency inventory and enforces the license policy.
 *
 * Rust: resolved registry packages from `cargo metadata --locked`.
 * Node: production dependencies reported by `pnpm licenses list --prod` for the two
 * shipped packages (`apps/desktop` and `mcp/bridge`).
 *
 * MPL-2.0, Unicode-3.0 and CDLA-Permissive-2.0 are accepted with notices; copyleft
 * licenses (GPL/AGPL/SSPL) and non-commercial licenses are rejected.
 */

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const ROOT = process.cwd();
const INVENTORY_JSON = resolve(ROOT, "docs/legal/third-party-inventory.json");
const INVENTORY_MD = resolve(ROOT, "docs/legal/third-party-inventory.md");
const CHECK_ONLY = process.argv.includes("--check");

const ALLOWED = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "CC0-1.0",
  "CDLA-Permissive-2.0",
  "ISC",
  "MIT",
  "MIT-0",
  "MPL-2.0",
  "Unicode-3.0",
  "Unlicense",
  "Zlib",
]);
const DENIED_MARKERS = [
  "AGPL",
  "GPL",
  "SSPL",
  "BUSL",
  "Commons-Clause",
  "CC-BY-NC",
  "CC-BY-SA",
];
const NOTICE_REQUIRED = ["MPL-2.0", "Unicode-3.0", "CDLA-Permissive-2.0"];

function splitTopLevel(expression, separator) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < expression.length; index += 1) {
    const char = expression[index];
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    if (depth === 0 && expression.startsWith(separator, index)) {
      parts.push(expression.slice(start, index).trim());
      start = index + separator.length;
      index += separator.length - 1;
    }
  }
  if (parts.length > 0) {
    parts.push(expression.slice(start).trim());
  }
  return parts;
}

function stripOuterParens(expression) {
  let value = expression.trim();
  while (value.startsWith("(") && value.endsWith(")")) {
    let depth = 0;
    for (let index = 0; index < value.length; index += 1) {
      if (value[index] === "(") depth += 1;
      if (value[index] === ")") depth -= 1;
      if (depth === 0 && index < value.length - 1) {
        return value;
      }
    }
    value = value.slice(1, -1).trim();
  }
  return value;
}

function isAllowed(expression) {
  const value = stripOuterParens(expression);
  const orParts = splitTopLevel(value, " OR ");
  if (orParts.length > 1) {
    return orParts.some(isAllowed);
  }
  const slashParts = splitTopLevel(value, "/");
  if (slashParts.length > 1) {
    return slashParts.some(isAllowed);
  }
  const andParts = splitTopLevel(value, " AND ");
  if (andParts.length > 1) {
    return andParts.every(isAllowed);
  }
  if (value.includes(" WITH ")) {
    return isAllowed(value.split(" WITH ")[0]);
  }
  return ALLOWED.has(value);
}

function containsDeniedMarker(expression) {
  const upper = expression.toUpperCase();
  return DENIED_MARKERS.some((marker) => upper.includes(marker.toUpperCase()));
}

function classify(ecosystem, name, version, expression) {
  if (!expression || expression.trim() === "" || expression === "UNKNOWN") {
    return {
      ecosystem,
      name,
      version,
      license: expression || "UNKNOWN",
      status: "unknown",
      reason: "missing license metadata",
    };
  }
  if (!isAllowed(expression)) {
    return {
      ecosystem,
      name,
      version,
      license: expression,
      status: "denied",
      reason: containsDeniedMarker(expression)
        ? "contains a denied copyleft/non-commercial license"
        : "no permitted license branch",
    };
  }
  return { ecosystem, name, version, license: expression, status: "allowed" };
}

function run(command, args, options = {}) {
  return execFileSync(command, args, {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 128 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  });
}

function pnpmLicenseJson(directory) {
  const args = ["--dir", directory, "licenses", "list", "--prod", "--json"];
  if (process.platform === "win32") {
    const commandLine = `pnpm --dir ${directory} licenses list --prod --json`;
    return run(process.env.ComSpec || "cmd.exe", [
      "/d",
      "/s",
      "/c",
      commandLine,
    ]);
  }
  return run("pnpm", args);
}

function rustPackages() {
  const metadata = JSON.parse(
    run("cargo", ["metadata", "--format-version", "1", "--locked"]),
  );
  return metadata.packages
    .filter((pkg) => pkg.source)
    .map((pkg) => ({
      name: pkg.name,
      version: pkg.version,
      license:
        pkg.license || (pkg.license_file ? "SEE LICENSE FILE" : "UNKNOWN"),
    }))
    .sort(
      (left, right) =>
        left.name.localeCompare(right.name) ||
        left.version.localeCompare(right.version),
    );
}

function pnpmPackages(directory) {
  const parsed = JSON.parse(pnpmLicenseJson(directory));
  const packages = [];
  for (const [license, entries] of Object.entries(parsed)) {
    for (const entry of entries) {
      for (const version of entry.versions) {
        packages.push({ name: entry.name, version, license });
      }
    }
  }
  return packages;
}

function countLicenses(packages) {
  const counts = {};
  for (const pkg of packages) {
    counts[pkg.license] = (counts[pkg.license] || 0) + 1;
  }
  return Object.fromEntries(
    Object.entries(counts).sort((left, right) =>
      left[0].localeCompare(right[0]),
    ),
  );
}

function noticeComponents(packages) {
  return packages.filter((pkg) =>
    NOTICE_REQUIRED.some((marker) =>
      pkg.license.toUpperCase().includes(marker.toUpperCase()),
    ),
  );
}

function markdown(inventory) {
  const lines = [
    "# Generated third-party inventory",
    "",
    "This file is generated by `pnpm licenses:update`. Do not edit it by hand.",
    "",
    `Generated: ${inventory.generatedAt}`,
    "",
    "## Summary",
    "",
    "| Ecosystem | Packages | License expressions |",
    "| --- | ---: | --- |",
    `| Rust (resolved registry crates) | ${inventory.rust.packageCount} | ${Object.keys(inventory.rust.licenses).length} |`,
    `| Node (shipped production packages) | ${inventory.node.packageCount} | ${Object.keys(inventory.node.licenses).length} |`,
    "",
    "## Notice-required components",
    "",
  ];
  if (inventory.noticeRequired.length === 0) {
    lines.push("None.");
  } else {
    lines.push(
      "| Ecosystem | Package | Version | License |",
      "| --- | --- | --- | --- |",
    );
    for (const item of inventory.noticeRequired) {
      lines.push(
        `| ${item.ecosystem} | \`${item.name}\` | ${item.version} | ${item.license} |`,
      );
    }
    lines.push(
      "",
      "MPL-2.0 components are used unmodified as dependencies. Their source is available from",
      "their published package archives and the upstream projects; FuryBuilder does not",
      "relicense or modify those files. Unicode-3.0 and CDLA-Permissive-2.0 components are",
      "data/license texts used under their respective terms.",
    );
  }
  lines.push(
    "",
    "## Full inventory",
    "",
    "Machine-readable packages, versions and license expressions:",
    "`docs/legal/third-party-inventory.json`.",
    "",
    "## Policy",
    "",
    "Allowed: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD, Zlib, Unicode-3.0,",
    "CC0-1.0, MIT-0, Unlicense, MPL-2.0, CDLA-Permissive-2.0, and Apache-2.0 WITH",
    "LLVM-exception. Copyleft (GPL/AGPL/SSPL) and non-commercial licenses are rejected.",
    "Dual-license expressions pass when at least one branch is allowed.",
    "",
  );
  return lines.join("\n");
}

function main() {
  const rust = rustPackages().map((pkg) =>
    classify("rust", pkg.name, pkg.version, pkg.license),
  );
  const node = [
    ...pnpmPackages("apps/desktop"),
    ...pnpmPackages("mcp/bridge"),
  ].map((pkg) => classify("node", pkg.name, pkg.version, pkg.license));
  const all = [...rust, ...node];
  const denied = all.filter((pkg) => pkg.status === "denied");
  const unknown = all.filter((pkg) => pkg.status === "unknown");

  const inventory = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    policy:
      "Permissive licenses only; MPL-2.0, Unicode-3.0 and CDLA-Permissive-2.0 require notices. Dual licenses pass with any allowed branch.",
    rust: {
      packageCount: rust.length,
      licenses: countLicenses(rust),
      packages: rust.map(({ name, version, license }) => ({
        name,
        version,
        license,
      })),
    },
    node: {
      packageCount: node.length,
      licenses: countLicenses(node),
      packages: node.map(({ name, version, license }) => ({
        name,
        version,
        license,
      })),
    },
    noticeRequired: noticeComponents(all).map(
      ({ ecosystem, name, version, license }) => ({
        ecosystem,
        name,
        version,
        license,
      }),
    ),
    denied,
    unknown,
    pass: denied.length === 0 && unknown.length === 0,
  };

  if (!inventory.pass) {
    console.error(
      `License policy failed: ${denied.length} denied, ${unknown.length} unknown component(s).`,
    );
    for (const item of [...denied, ...unknown]) {
      console.error(
        `  ${item.ecosystem} ${item.name}@${item.version}: ${item.license} (${item.reason})`,
      );
    }
    process.exitCode = 1;
    return;
  }

  if (CHECK_ONLY) {
    console.log(
      `License policy passed: ${rust.length} Rust and ${node.length} Node components; ${inventory.noticeRequired.length} notice-required.`,
    );
    return;
  }

  writeFileSync(
    INVENTORY_JSON,
    `${JSON.stringify(inventory, null, 2)}\n`,
    "utf8",
  );
  writeFileSync(INVENTORY_MD, markdown(inventory), "utf8");
  console.log(
    `Wrote ${rust.length} Rust and ${node.length} Node components; ${inventory.noticeRequired.length} notice-required.`,
  );
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
