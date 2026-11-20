import assert from "node:assert/strict";
import test from "node:test";
import {
  groupCommits,
  parseConventionalCommit,
  renderNotes,
} from "./release-notes.mjs";

const commits = [
  {
    hash: "1111111111111111111111111111111111111111",
    subject: "feat(editor): add signed update banner!",
    body: "BREAKING CHANGE: update notices replace the old about box",
  },
  {
    hash: "2222222222222222222222222222222222222222",
    subject: "fix(updater): keep projects during install",
    body: "",
  },
  {
    hash: "3333333333333333333333333333333333333333",
    subject: "chore(release): bump manifests",
    body: "",
  },
  {
    hash: "4444444444444444444444444444444444444444",
    subject: "Update docs without a conventional type",
    body: "",
  },
];

test("parses conventional commits and breaking changes", () => {
  const parsed = parseConventionalCommit(
    "feat(editor)!: add signed update banner",
    "BREAKING CHANGE: notices replace the old about box",
  );
  assert.equal(parsed.type, "feat");
  assert.equal(parsed.scope, "editor");
  assert.equal(parsed.description, "add signed update banner");
  assert.equal(parsed.breaking, true);
  assert.equal(parsed.breakingNote, "notices replace the old about box");
});

test("groups conventional commits and keeps internal types", () => {
  const { groups, breaking } = groupCommits(commits);
  assert.equal(groups.get("Features").length, 1);
  assert.equal(groups.get("Bug Fixes").length, 1);
  assert.equal(groups.get("Chores").length, 1);
  assert.equal(groups.get("Other Changes").length, 1);
  assert.equal(breaking.length, 1);
  assert.equal(
    [...groups.values()]
      .flat()
      .some((commit) => commit.subject.startsWith("chore")),
    true,
  );
});

test("renders sections and links without install or checksum guidance", () => {
  const notes = renderNotes(commits, {
    tag: "v0.2.0",
    previousTag: "v0.1.0",
    repository: "ahmikael/FuryBuilder",
  });
  assert.match(notes, /## What's Changed/);
  assert.match(notes, /### Breaking Changes/);
  assert.match(notes, /### Features/);
  assert.match(notes, /### Bug Fixes/);
  assert.match(notes, /### Chores/);
  assert.match(notes, /### Other Changes/);
  assert.match(notes, /bump manifests/);
  assert.match(notes, /compare\/v0\.1\.0\.\.\.v0\.2\.0/);
  assert.doesNotMatch(notes, /## Installing/);
  assert.doesNotMatch(notes, /SHA-256/);
  assert.doesNotMatch(notes, /MSI/);
});

test("keeps every conventional commit type in its own section", () => {
  const samples = [
    ["build(deps): refresh lockfile", "Build System"],
    ["test(editor): cover undo", "Tests"],
    ["ci(release): pin actions", "Continuous Integration"],
    ["style(ui): format settings", "Code Style"],
    ["chore(release): bump manifests", "Chores"],
  ];
  const { groups } = groupCommits(
    samples.map(([subject], index) => ({
      hash: String(index + 1).padStart(40, "0"),
      subject,
      body: "",
    })),
  );
  for (const [subject, title] of samples) {
    assert.equal(groups.get(title).length, 1, `${subject} should be in ${title}`);
    assert.equal(groups.get("Other Changes").length, 0);
  }
  assert.equal(
    parseConventionalCommit("chore(release): bump manifests").type,
    "chore",
  );
});
