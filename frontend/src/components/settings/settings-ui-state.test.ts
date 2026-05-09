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

  it("migrates legacy Settings default sections to Providers", () => {
    expect(migrateActiveSectionPreference("execution", 0)).toBe("providers");
    expect(migrateActiveSectionPreference("repository", 1)).toBe("providers");
    expect(migrateActiveSectionPreference(null, 0)).toBe("providers");
  });

  it("preserves explicit non-default section choices during migration", () => {
    expect(migrateActiveSectionPreference("review", 0)).toBe("review");
  });

  it("preserves current-version section choices", () => {
    expect(migrateActiveSectionPreference("execution", 2)).toBe("execution");
  });

  it("loads Providers and writes the migrated active-section version", () => {
    localStorage.setItem("ralphx-settings-active-section", "execution");

    expect(loadActiveSection()).toBe("providers");
    expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
      "providers",
    );
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "2",
    );
  });

  it("saves explicit user choices at the current preference version", () => {
    saveActiveSection("review");

    expect(loadActiveSection()).toBe("review");
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "2",
    );
  });
});
