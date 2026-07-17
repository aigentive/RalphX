import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Bot, ChevronDown, ChevronRight, Search } from "lucide-react";

import type { ManualRoleCatalogEntry } from "@/api/manual-role-defaults.types";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useManualRoleDefaults } from "@/hooks/useManualRoleDefaults";
import { fetchPersonas, personaKeys } from "@/hooks/usePersonas";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

import { AgentRoleDefaultRow } from "./AgentRoleDefaultRow";
import { ErrorBanner, SectionCard } from "./SettingsView.shared";

type Scope = "global" | "project";

interface FamilyGroup {
  id: string;
  label: string;
  roles: ManualRoleCatalogEntry[];
}

export function AgentsSettingsSection() {
  const activeProject = useProjectStore(selectActiveProject);
  const openModal = useUiStore((state) => state.openModal);
  const [scope, setScope] = useState<Scope>("global");
  const [search, setSearch] = useState("");
  const [collapsedFamilies, setCollapsedFamilies] = useState<Set<string>>(
    () => new Set(),
  );
  const projectId = scope === "project" ? activeProject?.id ?? null : null;
  const defaults = useManualRoleDefaults(projectId);
  const { registry } = useAgentModels();
  const { providers } = useHarnessProviders();
  const { data: featureFlags } = useFeatureFlags();
  const personasQuery = useQuery({
    queryKey: personaKeys.list(),
    queryFn: fetchPersonas,
    enabled: featureFlags.agentPersonas ?? false,
  });

  const providerIds = useMemo(() => {
    const enabled = providers.filter((provider) => provider.enabled).map((provider) => provider.provider);
    return enabled.length > 0 ? enabled : ["claude", "codex"];
  }, [providers]);
  const families = useMemo<FamilyGroup[]>(() => {
    const normalizedSearch = search.trim().toLowerCase();
    const groups = new Map<string, FamilyGroup>();
    for (const role of defaults.catalog?.roles ?? []) {
      if (
        normalizedSearch &&
        !`${role.displayName} ${role.role} ${role.familyDisplayName}`
          .toLowerCase()
          .includes(normalizedSearch)
      ) {
        continue;
      }
      const group = groups.get(role.family) ?? {
        id: role.family,
        label: role.familyDisplayName,
        roles: [],
      };
      group.roles.push(role);
      groups.set(role.family, group);
    }
    return [...groups.values()];
  }, [defaults.catalog?.roles, search]);
  const activePersonas = (personasQuery.data ?? []).filter(
    (persona) => persona.status === "active",
  );

  const toggleFamily = (family: string) => {
    setCollapsedFamilies((current) => {
      const next = new Set(current);
      if (next.has(family)) next.delete(family);
      else next.add(family);
      return next;
    });
  };

  return (
    <SectionCard
      icon={<Bot className="h-5 w-5" />}
      title="Agents"
      description="Configure the Manual default used by each backend-owned agent role."
    >
      <div className="space-y-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <Tabs value={scope} onValueChange={(value) => setScope(value as Scope)}>
            <TabsList aria-label="Agent default scope">
              <TabsTrigger value="global">Global Defaults</TabsTrigger>
              <TabsTrigger value="project" disabled={!activeProject}>
                Project Overrides
              </TabsTrigger>
            </TabsList>
          </Tabs>
          <label className="relative min-w-[220px] flex-1 sm:max-w-xs">
            <span className="sr-only">Search agent roles</span>
            <Search className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-[var(--text-muted)]" />
            <input
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder="Search roles"
              className="settings-input h-9 w-full pl-8"
            />
          </label>
        </div>

        {scope === "project" && activeProject && (
          <p className="text-xs text-[var(--text-muted)]">
            Overrides for {activeProject.name}. Follow removes the UI row and reveals the next configured source.
          </p>
        )}
        {defaults.isError && (
          <ErrorBanner
            error={defaults.error instanceof Error ? defaults.error.message : "Failed to load agent defaults"}
            onDismiss={() => undefined}
          />
        )}
        {defaults.isLoading && (
          <div data-testid="agents-settings-loading" className="space-y-3" aria-label="Loading agent defaults">
            <div className="h-10 rounded-md bg-[var(--bg-hover)]" />
            <div className="h-28 rounded-md bg-[var(--bg-surface)]" />
          </div>
        )}

        {!defaults.isLoading && families.map((family) => {
          const collapsed = collapsedFamilies.has(family.id) && !search;
          return (
            <section key={family.id} aria-labelledby={`agent-family-${family.id}`}>
              <button
                type="button"
                onClick={() => toggleFamily(family.id)}
                aria-expanded={!collapsed}
                className="flex w-full items-center justify-between rounded-md px-2 py-2 text-left hover:bg-[var(--bg-hover)]"
              >
                <span id={`agent-family-${family.id}`} className="font-semibold text-[var(--text-primary)]">
                  {family.label} <span className="text-xs font-normal text-[var(--text-muted)]">({family.roles.length})</span>
                </span>
                {collapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
              </button>
              {!collapsed && (
                <div className="mt-2 space-y-3">
                  {family.roles.map((entry) => (
                    <AgentRoleDefaultRow
                      key={entry.role}
                      entry={entry}
                      disabled={defaults.isSaving}
                      providers={providerIds}
                      modelsForProvider={(provider) =>
                        provider === "codex" || provider === "claude" ? registry[provider] : []
                      }
                      personas={activePersonas}
                      onUpdate={(value) => defaults.updateDefault(entry.role, value)}
                      onFollow={() => defaults.clearDefault(entry.role)}
                      onManagePersonas={() => openModal("settings", { section: "personas" })}
                    />
                  ))}
                </div>
              )}
            </section>
          );
        })}
        {!defaults.isLoading && families.length === 0 && (
          <p className="py-8 text-center text-sm text-[var(--text-muted)]">No agent roles match this search.</p>
        )}
      </div>
    </SectionCard>
  );
}
