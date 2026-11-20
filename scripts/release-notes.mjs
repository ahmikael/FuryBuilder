#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const TAG_PATTERN = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const CONVENTIONAL_PATTERN =
  /^([a-zA-Z][a-zA-Z0-9]*)(?:\(([^)\r\n]+)\))?(!)?:\s+(.+)$/;

const TYPE_SECTIONS = [
  ["feat", "Features"],
  ["fix", "Bug Fixes"],
  ["perf", "Performance Improvements"],
  ["refactor", "Refactoring"],
  ["docs", "Documentation"],
  ["build", "Build System"],
  ["test", "Tests"],
  ["ci", "Continuous Integration"],
  ["style", "Code Style"],
  ["chore", "Chores"],
  ["revert", "Reverts"],
];

const SECTION_TITLES = new Map(TYPE_SECTIONS);

export function parseConventionalCommit(subject, body = "") {
  const match = CONVENTIONAL_PATTERN.exec(subject.trim());
  if (!match) return null;
  const [, rawType, rawScope, bang, rawDescription] = match;
  const type = rawType.toLowerCase();
  const breakingMatch = /^BREAKING[ -]CHANGE:\s*(.+)$/m.exec(body);
  return {
    type: SECTION_TITLES.has(type) ? type : "other",
    scope: rawScope?.trim() || null,
    description: rawDescription.trim(),
    breaking: Boolean(bang) || Boolean(breakingMatch),
    breakingNote: breakingMatch?.[1].trim() ?? null,
  };
}

export function groupCommits(commits) {
  const groups = new Map(
    [...SECTION_TITLES.values(), "Other Changes"].map((title) => [title, []]),
  );
  const breaking = [];

  for (const commit of commits) {
    const parsed = parseConventionalCommit(commit.subject, commit.body);
    if (!parsed) {
      groups
        .get("Other Changes")
        .push({ ...commit, scope: null, description: commit.subject });
      continue;
    }
    const title = SECTION_TITLES.get(parsed.type) ?? "Other Changes";
    groups.get(title).push({ ...commit, ...parsed });
    if (parsed.breaking) {
      breaking.push({ ...commit, ...parsed });
    }
  }

  return { groups, breaking };
}

function commitLink(commit, repository) {
  if (!repository) return commit.hash.slice(0, 7);
  return `[${commit.hash.slice(0, 7)}](https://github.com/${repository}/commit/${commit.hash})`;
}

function formatEntry(commit, repository) {
  const scope = commit.scope ? `**${commit.scope}:** ` : "";
  return `- ${scope}${commit.description} (${commitLink(commit, repository)})`;
}

export function renderNotes(commits, options = {}) {
  const {
    tag,
    previousTag = null,
    repository = null,
  } = options;
  const { groups, breaking } = groupCommits(commits);
  const lines = ["## What's Changed", ""];

  if (breaking.length > 0) {
    lines.push("### Breaking Changes", "");
    for (const commit of breaking) {
      lines.push(formatEntry(commit, repository));
      if (commit.breakingNote) lines.push(`  - ${commit.breakingNote}`);
    }
    lines.push("");
  }

  for (const [, title] of TYPE_SECTIONS) {
    const entries = groups.get(title);
    if (entries.length === 0) continue;
    lines.push(`### ${title}`, "");
    for (const commit of entries) lines.push(formatEntry(commit, repository));
    lines.push("");
  }

  const other = groups.get("Other Changes");
  if (other.length > 0) {
    lines.push("### Other Changes", "");
    for (const commit of other) lines.push(formatEntry(commit, repository));
    lines.push("");
  }

  if (lines.length === 2) {
    lines.push("No user-facing changes in this release.", "");
  }

  if (previousTag) {
    lines.push(
      `**Full Changelog**: https://github.com/${repository || "ahmikael/FuryBuilder"}/compare/${previousTag}...${tag}`,
      "",
    );
  }

  return lines.join("\n").trimEnd() + "\n";
}

function git(args, options = {}) {
  return execFileSync("git", args, {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  }).trim();
}

export function previousReleaseTag(tag) {
  try {
    return (
      git(
        ["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", `${tag}^`],
        {
          stdio: ["ignore", "pipe", "ignore"],
        },
      ) || null
    );
  } catch {
    return null;
  }
}

export function collectCommits(range) {
  const output = git([
    "log",
    "--no-merges",
    "--format=%H%x1f%s%x1f%b%x1e",
    range,
  ]);
  if (!output) return [];
  return output
    .split("\x1e")
    .map((record) => record.trim())
    .filter(Boolean)
    .map((record) => {
      const [hash, subject, body = ""] = record.split("\x1f");
      return { hash, subject, body };
    });
}

function repositoryFromRemote() {
  try {
    const remote = git(["remote", "get-url", "origin"]);
    const match = /github\.com[:/]([^/]+\/[^/.]+)(?:\.git)?$/.exec(remote);
    return match?.[1] ?? null;
  } catch {
    return null;
  }
}

function parseArgs(argv) {
  const options = {
    tag: process.env.GITHUB_REF_NAME || null,
    to: "HEAD",
    from: null,
    output: null,
    repository: process.env.GITHUB_REPOSITORY || repositoryFromRemote(),
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    const next = argv[index + 1];
    switch (argument) {
      case "--tag":
        options.tag = next;
        index += 1;
        break;
      case "--to":
        options.to = next;
        index += 1;
        break;
      case "--from":
        options.from = next;
        index += 1;
        break;
      case "--output":
        options.output = next;
        index += 1;
        break;
      case "--repository":
        options.repository = next;
        index += 1;
        break;
      case "--help":
        console.log(
          "Usage: node scripts/release-notes.mjs --tag v1.2.3 [--output release-notes.md] [--from v1.2.2]",
        );
        process.exit(0);
        break;
      default:
        throw new Error(`Unknown argument: ${argument}`);
    }
  }
  if (!options.tag)
    throw new Error("A release tag is required (--tag vX.Y.Z).");
  if (!TAG_PATTERN.test(options.tag)) {
    throw new Error(`Release tags must look like vX.Y.Z, got ${options.tag}.`);
  }
  return options;
}

export function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  const version = JSON.parse(
    readFileSync(resolve("version.json"), "utf8"),
  ).version;
  if (options.tag !== `v${version}`) {
    throw new Error(
      `Tag ${options.tag} does not match version.json (${version}). Run pnpm version:set before tagging.`,
    );
  }

  const previousTag = options.from ?? previousReleaseTag(options.tag);
  const range = previousTag ? `${previousTag}..${options.to}` : options.to;
  const commits = collectCommits(range);
  const notes = renderNotes(commits, {
    tag: options.tag,
    previousTag,
    repository: options.repository,
  });

  if (options.output) {
    writeFileSync(resolve(options.output), notes, "utf8");
  } else {
    process.stdout.write(notes);
  }
  return { notes, commits, previousTag };
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  try {
    main();
  } catch (error) {
    console.error(
      `release-notes: ${error instanceof Error ? error.message : error}`,
    );
    process.exit(1);
  }
}
