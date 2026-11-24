import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const SETTINGS_DIR = resolve(process.cwd(), "src/features/settings");

function settingsSources(): Array<{ name: string; source: string }> {
  return readdirSync(SETTINGS_DIR)
    .filter((name) => name.endsWith(".tsx") && !name.endsWith(".test.tsx"))
    .map((name) => ({
      name,
      source: readFileSync(resolve(SETTINGS_DIR, name), "utf8"),
    }));
}

/**
 * Settings is a maintenance surface, not a second project wizard. These checks fail closed
 * when a future change imports provisioning or project-creation entry points into it.
 */
describe("settings boundaries", () => {
  it("contains no project creation or component provisioning entry points", () => {
    const forbidden = [
      "ProjectWizard",
      "ProjectComponentsStep",
      "VanillaWorldWizard",
      "@/lib/project-api",
      "@/lib/server-api",
      "installComponent",
      "relinkComponent",
      "selectPlaceholderComponent",
      "componentJavaRelease",
      "componentAssetsRelease",
      "componentServerRelease",
      "acceptServerEula",
      "startServerGeneration",
    ];

    for (const { name, source } of settingsSources()) {
      for (const token of forbidden) {
        expect(source, `${name} must not contain ${token}`).not.toContain(token);
      }
    }
  });

  it("keeps the Components tab to inventory maintenance", () => {
    const dialog = readFileSync(resolve(SETTINGS_DIR, "SettingsDialog.tsx"), "utf8");

    expect(dialog).toContain("<ComponentInventorySection />");
    expect(dialog).toContain("chosen in the project wizard");
    expect(dialog).not.toContain("Create project");
    expect(dialog).not.toContain("Start generation");
    expect(dialog).not.toContain("Accept EULA");
  });
});
