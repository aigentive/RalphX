import { beforeEach, describe, expect, it } from "vitest";

import {
  agentsDisclosureScope,
  loadAgentsDisclosure,
  loadAgentsDisclosures,
  loadAgentsTab,
  loadActiveSection,
  loadActiveDestination,
  migrateActiveSectionPreference,
  migrateSettingsUiState,
  saveAgentsFamiliesExpanded,
  saveAgentsFamilyExpanded,
  saveAgentsRoleExpanded,
  saveAgentsTab,
  saveActiveSection,
} from "./settings-ui-state";

describe("settings-ui-state", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("maps legacy standalone sections to parent sections and tabs", () => {
    expect(migrateActiveSectionPreference("execution", 3)).toEqual({ section: "workspace", tab: "general" });
    expect(migrateActiveSectionPreference("review", 3)).toEqual({ section: "tasks", tab: "review-policy" });
    expect(migrateActiveSectionPreference("autonomy", 3)).toEqual({ section: "tasks", tab: "autonomy-policy" });
    expect(migrateActiveSectionPreference("workspace-review", 3)).toEqual({ section: "workspace", tab: "review" });
    expect(migrateActiveSectionPreference("global-execution", 3)).toEqual({ section: "capacity" });
    expect(migrateActiveSectionPreference("ideation-workflow", 3)).toEqual({ section: "tasks", tab: "general" });
  });

  it("defaults missing legacy preferences to Providers", () => {
    expect(migrateActiveSectionPreference(null, 0)).toEqual({ section: "providers" });
  });

  it("preserves current-version section choices", () => {
    expect(migrateActiveSectionPreference("planning", 4)).toEqual({ section: "planning" });
  });

  it("migrates saved Execution and Ideation agent pages into Agents", () => {
    expect(migrateActiveSectionPreference("execution-harnesses", 2)).toEqual({ section: "agents" });
    expect(migrateActiveSectionPreference("ideation-harnesses", 2)).toEqual({ section: "agents" });
  });

  it("loads a legacy tab deterministically before persisting its parent section", () => {
    localStorage.setItem("ralphx-settings-active-section", "execution");

    expect(loadActiveDestination()).toEqual({ section: "workspace", tab: "general" });
    expect(loadActiveDestination()).toEqual({ section: "workspace", tab: "general" });

    migrateSettingsUiState();

    expect(loadActiveSection()).toBe("workspace");
    expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
      "workspace",
    );
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "4",
    );
  });

  it("saves explicit user choices at the current preference version", () => {
    saveActiveSection("tasks");

    expect(loadActiveSection()).toBe("tasks");
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "4",
    );
  });

  it("falls back from a saved project tab without overwriting the preference", () => {
    saveAgentsTab("project");

    expect(loadAgentsTab(false)).toBe("global");
    expect(loadAgentsTab(true)).toBe("project");
  });

  it("isolates family and role disclosure by global and project id", () => {
    const globalScope = agentsDisclosureScope(null);
    const projectOne = agentsDisclosureScope("project-1");
    const projectTwo = agentsDisclosureScope("project-2");

    saveAgentsFamilyExpanded(globalScope, "workspace", true);
    saveAgentsFamiliesExpanded(globalScope, ["automation", "execution"], true);
    saveAgentsRoleExpanded(projectOne, "workspace_edit", true);

    expect(loadAgentsDisclosure(globalScope).families.workspace).toBe(true);
    expect(loadAgentsDisclosure(globalScope).families).toMatchObject({
      automation: true,
      execution: true,
    });
    expect(loadAgentsDisclosure(globalScope).roles.workspace_edit).toBeUndefined();
    expect(loadAgentsDisclosure(projectOne).roles.workspace_edit).toBe(true);
    expect(loadAgentsDisclosure(projectTwo)).toEqual({ families: {}, roles: {} });
    expect(loadAgentsDisclosures()[projectOne]?.roles.workspace_edit).toBe(true);
  });

  it("fails safely when the persisted Agents disclosure record is malformed", () => {
    localStorage.setItem("ralphx-settings-agents-state", "{broken");
    expect(loadAgentsTab(true)).toBe("global");
    expect(loadAgentsDisclosure("global")).toEqual({ families: {}, roles: {} });

    localStorage.setItem(
      "ralphx-settings-agents-state",
      JSON.stringify({ version: 1, activeTab: "project", disclosures: { global: { families: { workspace: "yes" } } } }),
    );
    expect(loadAgentsDisclosure("global")).toEqual({ families: {}, roles: {} });
  });
});
