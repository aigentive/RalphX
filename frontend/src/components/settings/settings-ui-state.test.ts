import { beforeEach, describe, expect, it } from "vitest";

import {
  agentsDisclosureScope,
  loadAgentsDisclosure,
  loadAgentsDisclosures,
  loadAgentsTab,
  loadActiveSection,
  migrateActiveSectionPreference,
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

  it("migrates legacy Settings default sections to Providers", () => {
    expect(migrateActiveSectionPreference("execution", 0)).toBe("providers");
    expect(migrateActiveSectionPreference("repository", 1)).toBe("providers");
    expect(migrateActiveSectionPreference(null, 0)).toBe("providers");
  });

  it("preserves explicit non-default section choices during migration", () => {
    expect(migrateActiveSectionPreference("review", 0)).toBe("review");
  });

  it("preserves current-version section choices", () => {
    expect(migrateActiveSectionPreference("execution", 3)).toBe("execution");
  });

  it("migrates saved Execution and Ideation agent pages into Agents", () => {
    expect(migrateActiveSectionPreference("execution-harnesses", 2)).toBe("agents");
    expect(migrateActiveSectionPreference("ideation-harnesses", 2)).toBe("agents");
  });

  it("loads Providers and writes the migrated active-section version", () => {
    localStorage.setItem("ralphx-settings-active-section", "execution");

    expect(loadActiveSection()).toBe("providers");
    expect(localStorage.getItem("ralphx-settings-active-section")).toBe(
      "providers",
    );
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "3",
    );
  });

  it("saves explicit user choices at the current preference version", () => {
    saveActiveSection("review");

    expect(loadActiveSection()).toBe("review");
    expect(localStorage.getItem("ralphx-settings-active-section-version")).toBe(
      "3",
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
