import { beforeEach, describe, expect, it } from "vitest";

import {
  loadActiveSection,
  migrateActiveSectionPreference,
  saveActiveSection,
} from "./settings-ui-state";

describe("settings-ui-state", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("migrates the legacy Settings default section to Repository", () => {
    expect(migrateActiveSectionPreference("execution", 0)).toBe("repository");
    expect(migrateActiveSectionPreference(null, 0)).toBe("repository");
  });

  it("preserves explicit non-default section choices during migration", () => {
    expect(migrateActiveSectionPreference("review", 0)).toBe("review");
  });

  it("preserves current-version section choices", () => {
    expect(migrateActiveSectionPreference("execution", 1)).toBe("execution");
  });

  it("loads Repository and writes the migrated active-section version", () => {
    localStorage.setItem("ralphx-settings-active-section", "execution");

    expect(loadActiveSection()).toBe("repository");
    expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
      "repository",
    );
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "1",
    );
  });

  it("saves explicit user choices at the current preference version", () => {
    saveActiveSection("review");

    expect(loadActiveSection()).toBe("review");
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "1",
    );
  });
});
