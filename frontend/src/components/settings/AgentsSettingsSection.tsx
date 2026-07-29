import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Bot, ChevronDown, ChevronRight, Search } from "lucide-react";

import type { ManualRoleCatalogEntry } from "@/api/manual-role-defaults.types";
import { AGENT_START_MODE_OPTIONS } from "@/components/agents/agentStartModeOptions";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useManualRoleDefaults } from "@/hooks/useManualRoleDefaults";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
import { fetchPersonas, personaKeys } from "@/hooks/usePersonas";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import {
  isAgentDefaultStartMode,
  useAgentSessionStore,
  type AgentDefaultStartMode,
} from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";

import { AgentRoleDefaultRow } from "./AgentRoleDefaultRow";
import type { AgentsTabValue } from "./settings-ui-state";
import {
  ErrorBanner,
  SectionCard,
  SelectSettingRow,
  type SelectOption,
} from "./SettingsView.shared";
import { useAgentsSettingsUiState } from "./useAgentsSettingsUiState";

interface FamilyGroup {
  id: string;
  label: string;
  roles: ManualRoleCatalogEntry[];
  totalCount: number;
  overrideCount: number;
  diagnosticCount: number;
}

const DEFAULT_START_MODE_OPTIONS: SelectOption<AgentDefaultStartMode>[] =
  AGENT_START_MODE_OPTIONS.flatMap((option) =>
    isAgentDefaultStartMode(option.id)
      ? [
          {
            value: option.id,
            label: option.label,
            description: option.description,
          },
        ]
      : [],
  );

function roleMatchesSearch(
  role: ManualRoleCatalogEntry,
  normalizedSearch: string,
): boolean {
  return !normalizedSearch || [
    role.displayName,
    role.description,
    role.role,
    role.familyDisplayName,
  ].some((value) => value.toLowerCase().includes(normalizedSearch));
}

export function AgentsSettingsSection() {
  const activeProject = useProjectStore(selectActiveProject);
  const openModal = useUiStore((state) => state.openModal);
  const defaultStartMode = useAgentSessionStore(
    (state) => state.defaultStartMode,
  );
  const setDefaultStartMode = useAgentSessionStore(
    (state) => state.setDefaultStartMode,
  );
  const [search, setSearch] = useState("");
  const [overridesOnly, setOverridesOnly] = useState(false);
  const [attentionOnly, setAttentionOnly] = useState(false);
  const agentsUi = useAgentsSettingsUiState(activeProject?.id ?? null);
  const { scope, projectId, disclosure } = agentsUi;
  const defaults = useManualRoleDefaults(projectId);
  const ideationSettings = useIdeationSettings();
  const tasksEnabled =
    !ideationSettings.isLoading &&
    !ideationSettings.isError &&
    ideationSettings.settings.tasksFeatureState === "enabled";
  const { registry } = useAgentModels();
  const { providers } = useHarnessProviders();
  const { data: featureFlags } = useFeatureFlags();
  const personasQuery = useQuery({
    queryKey: personaKeys.list(),
    queryFn: () => fetchPersonas(),
    enabled: featureFlags.agentPersonas ?? false,
  });

  const providerIds = useMemo(() => {
    const enabled = providers
      .filter((provider) => provider.enabled)
      .map((provider) => provider.provider);
    return enabled.length > 0 ? enabled : ["claude", "codex"];
  }, [providers]);
  const families = useMemo<FamilyGroup[]>(() => {
    const normalizedSearch = search.trim().toLowerCase();
    const groups = new Map<string, FamilyGroup>();
    for (const role of defaults.catalog?.roles ?? []) {
      if (!tasksEnabled && role.requiresTasks) continue;
      const existing = groups.get(role.family);
      const group = existing ?? {
        id: role.family,
        label: role.familyDisplayName,
        roles: [],
        totalCount: 0,
        overrideCount: 0,
        diagnosticCount: 0,
      };
      group.totalCount += 1;
      if (role.configured) group.overrideCount += 1;
      if (role.diagnostics.length > 0) group.diagnosticCount += 1;
      if (
        roleMatchesSearch(role, normalizedSearch) &&
        (!overridesOnly || role.configured !== null) &&
        (!attentionOnly || role.diagnostics.length > 0)
      ) {
        group.roles.push(role);
      }
      if (!existing) groups.set(role.family, group);
    }
    const filtering = Boolean(normalizedSearch || overridesOnly || attentionOnly);
    return [...groups.values()].filter(
      (group) => !filtering || group.roles.length > 0,
    );
  }, [attentionOnly, defaults.catalog?.roles, overridesOnly, search, tasksEnabled]);
  const activePersonas = (personasQuery.data ?? []).filter(
    (persona) => persona.status === "active",
  );
  const filtersForceVisibility = Boolean(
    search.trim() || overridesOnly || attentionOnly,
  );
  const allFamiliesExpanded =
    families.length > 0 &&
    families.every((family) => disclosure.families[family.id] === true);

  const selectScope = (nextScope: AgentsTabValue) => {
    if (defaults.isSaving) return;
    defaults.dismissSaveError();
    agentsUi.setScope(nextScope);
  };

  return (
    <SectionCard
      icon={<Bot className="h-5 w-5" />}
      title="Role defaults"
      description="Configure new-run behavior and backend-owned defaults for every agent role."
    >
      <div className="agents-settings-content space-y-4">
        <SelectSettingRow
          id="agent-default-start-mode"
          label="Default new-run mode"
          description="Choose the mode selected when you open Agents > New run."
          value={defaultStartMode}
          options={DEFAULT_START_MODE_OPTIONS}
          disabled={false}
          onChange={setDefaultStartMode}
        />
        {!tasksEnabled && (
          <p
            data-testid="tasks-required-roles-hidden-notice"
            className="rounded-md px-3 py-2 text-xs"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
          >
            Execution roles are hidden while Tasks are off. Saved overrides are preserved.
          </p>
        )}
        <div className="agents-settings-toolbar">
          <Tabs
            value={scope}
            onValueChange={(value) => selectScope(value as AgentsTabValue)}
          >
            <TabsList aria-label="Agent default scope">
              <TabsTrigger value="global" disabled={defaults.isSaving}>
                Global Defaults
              </TabsTrigger>
              <TabsTrigger
                value="project"
                disabled={!activeProject || defaults.isSaving}
              >
                Project Overrides
              </TabsTrigger>
            </TabsList>
          </Tabs>
          <label className="relative min-w-[220px] flex-1 sm:max-w-xs">
            <span className="sr-only">Search agent roles</span>
            <Search
              aria-hidden="true"
              className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-[var(--text-muted)]"
            />
            <input
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder="Search roles"
              className="settings-input h-9 w-full pl-8 font-[var(--font-body)]"
            />
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              aria-pressed={overridesOnly}
              onClick={() => setOverridesOnly((current) => !current)}
              className="agents-filter-button"
            >
              Overrides only
            </button>
            <button
              type="button"
              aria-pressed={attentionOnly}
              onClick={() => setAttentionOnly((current) => !current)}
              className="agents-filter-button"
            >
              Needs attention
            </button>
            <button
              type="button"
              onClick={() =>
                agentsUi.setAllFamiliesExpanded(
                  families.map((family) => family.id),
                  !allFamiliesExpanded,
                )
              }
              className="agents-filter-button"
            >
              {allFamiliesExpanded ? "Collapse families" : "Expand families"}
            </button>
          </div>
        </div>

        {scope === "project" && activeProject && (
          <p className="text-xs text-[var(--text-muted)]">
            Overrides for {activeProject.name}. Using an inherited default
            removes only this project&apos;s UI override.
          </p>
        )}
        {defaults.isError && (
          <ErrorBanner
            error={
              defaults.error instanceof Error
                ? defaults.error.message
                : "Failed to load agent defaults"
            }
          />
        )}
        {defaults.saveError && (
          <ErrorBanner
            error={
              defaults.saveError instanceof Error
                ? defaults.saveError.message
                : "Failed to save agent default"
            }
            onDismiss={defaults.dismissSaveError}
          />
        )}
        {defaults.isLoading && (
          <div
            data-testid="agents-settings-loading"
            className="space-y-3"
            aria-label="Loading agent defaults"
          >
            <div className="h-10 rounded-md bg-[var(--bg-hover)]" />
            <div className="h-20 rounded-md bg-[var(--bg-surface)]" />
            <div className="h-20 rounded-md bg-[var(--bg-surface)]" />
          </div>
        )}

        {!defaults.isLoading &&
          !defaults.isError &&
          families.map((family) => {
            const forcedOpen =
              filtersForceVisibility || family.diagnosticCount > 0;
            const expanded =
              forcedOpen || disclosure.families[family.id] === true;
            return (
            <section
              key={family.id}
              data-testid="agent-family-row"
              className="agents-family"
              aria-labelledby={`agent-family-${family.id}`}
            >
              <button
                type="button"
                onClick={() => {
                  if (!forcedOpen) {
                    agentsUi.setFamilyExpanded(family.id, !expanded);
                  }
                }}
                aria-disabled={forcedOpen}
                aria-expanded={expanded}
                className="agents-family__summary"
              >
                <span className="flex min-w-0 items-center gap-2">
                  {expanded
                    ? <ChevronDown aria-hidden="true" className="h-4 w-4 shrink-0" />
                    : <ChevronRight aria-hidden="true" className="h-4 w-4 shrink-0" />}
                  <span
                    id={`agent-family-${family.id}`}
                    className="font-semibold text-[var(--text-primary)]"
                  >
                    {family.label}
                  </span>
                </span>
                <span className="flex flex-wrap items-center justify-end gap-2 text-[11px] text-[var(--text-muted)]">
                  <span>
                    {family.roles.length === family.totalCount
                      ? `${family.totalCount} roles`
                      : `${family.roles.length} of ${family.totalCount} roles`}
                  </span>
                  {family.overrideCount > 0 && (
                    <span className="agents-count-badge">
                      {family.overrideCount} configured
                    </span>
                  )}
                  {family.diagnosticCount > 0 && (
                    <span className="agents-attention-badge">
                      {family.diagnosticCount} attention
                    </span>
                  )}
                </span>
              </button>
              {expanded && (
                <div className="agents-family__roles">
                  {family.roles.map((entry) => (
                    <AgentRoleDefaultRow
                      key={entry.role}
                      entry={entry}
                      expanded={disclosure.roles[entry.role] === true}
                      disabled={defaults.isSaving}
                      providers={providerIds}
                      modelsForProvider={(provider) =>
                        provider === "codex" || provider === "claude"
                          ? registry[provider]
                          : []
                      }
                      personas={activePersonas}
                      onUpdate={(value) =>
                        defaults.updateDefault(entry.role, value)
                      }
                      onExpandedChange={(expandedRole) =>
                        agentsUi.setRoleExpanded(entry.role, expandedRole)
                      }
                      onUseInheritedDefault={() =>
                        defaults.clearDefaultAsync(entry.role)
                      }
                      onManagePersonas={() =>
                        openModal("settings", { section: "personas" })
                      }
                    />
                  ))}
                </div>
              )}
            </section>
            );
          })}
        {!defaults.isLoading && !defaults.isError && families.length === 0 && (
          <p className="py-8 text-center text-sm text-[var(--text-muted)]">
            No agent roles match these filters.
          </p>
        )}
      </div>
    </SectionCard>
  );
}
