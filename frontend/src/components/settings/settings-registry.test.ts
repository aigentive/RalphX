import { describe, expect, it } from "vitest";

import {
  DEFAULT_SETTINGS_SECTION,
  SETTINGS_NAV,
  SETTINGS_SECTIONS,
  isSettingsSectionId,
  navForSection,
  resolveSettingsDestination,
  resolveSettingsSectionId,
  sectionMeta,
  type SettingsSectionId,
} from "./settings-registry";

/** Every leaf id that existed before the nav consolidation. */
const LEGACY_LEAF_IDS: SettingsSectionId[] = [
  "providers",
  "agents",
  "models",
  "personas",
  "capabilities",
  "tasks",
  "planning",
  "workspace",
  "capacity",
  "repository",
  "project-analysis",
  "api-keys",
  "external-mcp",
  "integrations",
  "github",
  "linear",
  "clickup",
  "granola",
  "mcp",
  "updates",
  "accessibility",
  "notifications",
];

describe("settings-registry nav model", () => {
  it("exposes exactly the seven mock nav entries in order", () => {
    expect(SETTINGS_NAV.map((nav) => nav.id)).toEqual([
      "models-providers",
      "agents",
      "automation",
      "repository",
      "integrations",
      "notifications",
      "application",
    ]);
  });

  it("gives every nav entry copy, an icon and at least one leaf", () => {
    for (const nav of SETTINGS_NAV) {
      expect(nav.label.length).toBeGreaterThan(0);
      expect(nav.sublabel.length).toBeGreaterThan(0);
      expect(nav.description.length).toBeGreaterThan(0);
      expect(nav.icon).toBeTruthy();
      expect(nav.leaves.length).toBeGreaterThan(0);
    }
  });

  it("assigns every registered section to exactly one nav entry", () => {
    const assigned = SETTINGS_NAV.flatMap((nav) => nav.leaves);
    expect(new Set(assigned).size).toBe(assigned.length);
    expect(new Set(assigned)).toEqual(
      new Set(SETTINGS_SECTIONS.map((section) => section.id)),
    );
  });

  it("keeps every pre-consolidation leaf reachable from the nav", () => {
    for (const id of LEGACY_LEAF_IDS) {
      expect(navForSection(id).leaves).toContain(id);
    }
  });

  it("routes the integrations hub and its drill-in panels to the integrations nav", () => {
    expect(navForSection("integrations-hub").id).toBe("integrations");
    expect(navForSection("github").id).toBe("integrations");
    expect(navForSection("api-keys").id).toBe("integrations");
  });

  it("registers Database as an Application leaf", () => {
    expect(sectionMeta("database")?.label).toBe("Database");
    expect(navForSection("database").id).toBe("application");
    expect(navForSection("database").leaves).toContain("database");
  });

  it("marks the integrations nav with its hub leaf and no other nav with one", () => {
    const withHub = SETTINGS_NAV.filter((nav) => nav.hubLeaf);
    expect(withHub).toHaveLength(1);
    expect(withHub[0]?.id).toBe("integrations");
    expect(withHub[0]?.hubLeaf).toBe("integrations-hub");
  });

  it("falls back to the default section's nav for an unknown id", () => {
    expect(navForSection("not-a-section" as SettingsSectionId)).toBe(
      navForSection(DEFAULT_SETTINGS_SECTION),
    );
  });
});

describe("settings-registry section metadata", () => {
  it("gives every section a label and a page description", () => {
    for (const section of SETTINGS_SECTIONS) {
      expect(section.label.length).toBeGreaterThan(0);
      expect(section.description.length).toBeGreaterThan(0);
    }
  });

  it("resolves metadata by id and returns null for unknown ids", () => {
    expect(sectionMeta("providers")?.label).toBe("Providers");
    expect(sectionMeta("nope" as SettingsSectionId)).toBeNull();
  });

  it("recognises the additive integrations-hub id", () => {
    expect(isSettingsSectionId("integrations-hub")).toBe(true);
  });
});

describe("resolveSettingsDestination", () => {
  it("keeps every legacy alias mapping byte-identical", () => {
    expect(resolveSettingsDestination("review")).toEqual({
      section: "tasks",
      tab: "review-policy",
    });
    expect(resolveSettingsDestination("autonomy")).toEqual({
      section: "tasks",
      tab: "autonomy-policy",
    });
    expect(resolveSettingsDestination("ideation-workflow")).toEqual({
      section: "tasks",
      tab: "general",
    });
    expect(resolveSettingsDestination("workspace-review")).toEqual({
      section: "workspace",
      tab: "review",
    });
    expect(resolveSettingsDestination("execution")).toEqual({
      section: "workspace",
      tab: "general",
    });
    expect(resolveSettingsDestination("global-execution")).toEqual({
      section: "capacity",
    });
    expect(resolveSettingsDestination("execution-harnesses")).toEqual({
      section: "agents",
    });
    expect(resolveSettingsDestination("ideation-harnesses")).toEqual({
      section: "agents",
    });
  });

  it("routes the legacy integrations id to the hub", () => {
    expect(resolveSettingsDestination("integrations")).toEqual({
      section: "integrations-hub",
    });
  });

  it("routes the explicit atlassian alias straight to the Atlassian panel", () => {
    expect(resolveSettingsDestination("atlassian")).toEqual({
      section: "integrations",
    });
  });

  it("resolves every other pre-existing leaf id to itself", () => {
    for (const id of LEGACY_LEAF_IDS) {
      if (id === "integrations") continue;
      expect(resolveSettingsDestination(id)).toEqual({ section: id });
    }
  });

  it("rejects unknown values", () => {
    expect(resolveSettingsDestination("nope")).toBeNull();
    expect(resolveSettingsDestination(undefined)).toBeNull();
    expect(resolveSettingsDestination(42)).toBeNull();
    expect(resolveSettingsSectionId("nope")).toBeNull();
  });
});
