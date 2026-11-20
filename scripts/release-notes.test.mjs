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

test("groups user-facing commits and hides internal types by default", () => {
  const { groups, breaking } = groupCommits(commits);
  assert.equal(groups.get("Features").length, 1);
  assert.equal(groups.get("Bug Fixes").length, 1);
  assert.equal(groups.get("Other Changes").length, 1);
  assert.equal(breaking.length, 1);
  assert.equal(
    [...groups.values()]
      .flat()
      .some((commit) => commit.subject.startsWith("chore")),
    false,
  );
});

test("renders sections, links and installation guidance", () => {
  const notes = renderNotes(commits, {
    tag: "v0.2.0",
    previousTag: "v0.1.0",
    repository: "ahmikael/FuryBuilder",
  });
  assert.match(notes, /## What's Changed/);
  assert.match(notes, /### Breaking Changes/);
  assert.match(notes, /### Features/);
  assert.match(notes, /### Bug Fixes/);
  assert.match(notes, /### Other Changes/);
  assert.match(notes, /compare\/v0\.1\.0\.\.\.v0\.2\.0/);
  assert.match(notes, /NSIS installer/);
  assert.doesNotMatch(notes, /chore\(release\)/);
});

test("can include internal commits explicitly", () => {
  const { groups } = groupCommits(commits, { includeInternal: true });
  assert.equal(
    [...groups.values()]
      .flat()
      .some((commit) => commit.subject.startsWith("chore")),
    true,
  );
  assert.equal(
    parseConventionalCommit("chore(release): bump manifests").internal,
    true,
  );
});
