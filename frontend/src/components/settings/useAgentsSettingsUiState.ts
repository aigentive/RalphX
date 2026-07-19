import { useEffect, useRef, useState } from "react";

import { scheduleAfterPaint } from "./SettingsDialog.performance";
import {
  agentsDisclosureScope,
  loadAgentsDisclosures,
  loadAgentsTab,
  saveAgentsFamiliesExpanded,
  saveAgentsFamilyExpanded,
  saveAgentsRoleExpanded,
  saveAgentsTab,
  type AgentsDisclosure,
  type AgentsDisclosureScope,
  type AgentsTabValue,
} from "./settings-ui-state";

const EMPTY_DISCLOSURE: AgentsDisclosure = { families: {}, roles: {} };

export function useAgentsSettingsUiState(activeProjectId: string | null) {
  const [scope, setScopeState] = useState<AgentsTabValue>(
    () => loadAgentsTab(Boolean(activeProjectId)),
  );
  const [disclosures, setDisclosures] = useState(
    () => loadAgentsDisclosures(),
  );
  const previousActiveProjectId = useRef(activeProjectId);
  const projectId = scope === "project" ? activeProjectId : null;
  const disclosureScope = agentsDisclosureScope(projectId);
  const disclosure = disclosures[disclosureScope] ?? EMPTY_DISCLOSURE;

  useEffect(() => {
    const previousProjectId = previousActiveProjectId.current;
    previousActiveProjectId.current = activeProjectId;

    if (!activeProjectId && scope === "project") {
      setScopeState("global");
      return;
    }
    if (
      !previousProjectId &&
      activeProjectId &&
      scope === "global" &&
      loadAgentsTab(true) === "project"
    ) {
      setScopeState("project");
    }
  }, [activeProjectId, scope]);

  const updateDisclosure = (
    targetScope: AgentsDisclosureScope,
    update: (current: AgentsDisclosure) => AgentsDisclosure,
  ) => {
    setDisclosures((current) => ({
      ...current,
      [targetScope]: update(current[targetScope] ?? EMPTY_DISCLOSURE),
    }));
  };

  const setScope = (nextScope: AgentsTabValue) => {
    if (nextScope === "project" && !activeProjectId) return;
    setScopeState(nextScope);
    scheduleAfterPaint(() => saveAgentsTab(nextScope));
  };

  const setFamilyExpanded = (family: string, expanded: boolean) => {
    updateDisclosure(disclosureScope, (current) => ({
      ...current,
      families: { ...current.families, [family]: expanded },
    }));
    scheduleAfterPaint(() => {
      saveAgentsFamilyExpanded(disclosureScope, family, expanded);
    });
  };

  const setRoleExpanded = (role: string, expanded: boolean) => {
    updateDisclosure(disclosureScope, (current) => ({
      ...current,
      roles: { ...current.roles, [role]: expanded },
    }));
    scheduleAfterPaint(() => {
      saveAgentsRoleExpanded(disclosureScope, role, expanded);
    });
  };

  const setAllFamiliesExpanded = (
    families: readonly string[],
    expanded: boolean,
  ) => {
    updateDisclosure(disclosureScope, (current) => ({
      ...current,
      families: {
        ...current.families,
        ...Object.fromEntries(families.map((family) => [family, expanded])),
      },
    }));
    scheduleAfterPaint(() => {
      saveAgentsFamiliesExpanded(disclosureScope, families, expanded);
    });
  };

  return {
    scope,
    projectId,
    disclosure,
    setScope,
    setFamilyExpanded,
    setRoleExpanded,
    setAllFamiliesExpanded,
  };
}
