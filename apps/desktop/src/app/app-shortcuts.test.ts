import { describe, expect, it } from "vitest";

import { appShortcut } from "@/app/app-shortcuts";

const base = {
  code: "",
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
};

describe("app shortcuts", () => {
  it("opens the command palette with Ctrl+K or Cmd+K", () => {
    expect(appShortcut({ ...base, code: "KeyK", ctrlKey: true })).toEqual({
      kind: "command-palette",
    });
    expect(appShortcut({ ...base, code: "KeyK", metaKey: true })).toEqual({
      kind: "command-palette",
    });
  });

  it("toggles the three panels", () => {
    expect(appShortcut({ ...base, code: "KeyB", ctrlKey: true })).toEqual({
      kind: "toggle-panel",
      panel: "left",
    });
    expect(appShortcut({ ...base, code: "KeyJ", ctrlKey: true })).toEqual({
      kind: "toggle-panel",
      panel: "bottom",
    });
    expect(appShortcut({ ...base, code: "KeyB", ctrlKey: true, altKey: true })).toEqual({
      kind: "toggle-panel",
      panel: "right",
    });
  });

  it("ignores plain keys, unrelated combinations and shifted palette keys", () => {
    expect(appShortcut({ ...base, code: "KeyK" })).toBeNull();
    expect(appShortcut({ ...base, code: "KeyK", ctrlKey: true, shiftKey: true })).toBeNull();
    expect(appShortcut({ ...base, code: "KeyX", ctrlKey: true })).toBeNull();
    expect(appShortcut({ ...base, code: "KeyB", altKey: true })).toBeNull();
  });
});
