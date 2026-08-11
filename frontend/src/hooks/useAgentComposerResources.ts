import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import {
  agentComposerApi,
  type AgentComposerEntry,
  type AgentComposerPlanReference,
  type AgentComposerSkill,
} from "@/api/agent-composer";
import {
  atlassianApi,
  type AtlassianResourceKind,
  type AtlassianResourceSummary,
} from "@/api/atlassian";
import { clickupApi, type ClickUpTaskSummary } from "@/api/clickup";
import { granolaApi, type GranolaNoteSummary } from "@/api/granola";
import { linearApi, type LinearIssueSummary } from "@/api/linear";
import type { AgentComposerIntegrationKind } from "@/components/agents/composer/agentComposerCore";
import {
  atlassianIntegrationKeys,
  isConfluenceConnected,
} from "@/hooks/useAtlassianIntegration";
import {
  granolaIntegrationKeys,
  isGranolaConnected,
} from "@/hooks/useGranolaIntegration";
import { useTicketingProviders } from "@/hooks/useTicketing";
import { getValidTicketingProviders } from "@/lib/ticketing-provider-state";

const CLICKUP_TASK_SEARCH_LIMIT = 10;
const CLICKUP_TASK_SEARCH_DEBOUNCE_MS = 1_000;
const INTEGRATION_SETTINGS_STALE_TIME_MS = 15_000;

export type AgentComposerIntegrationAvailability = Record<
  AgentComposerIntegrationKind,
  boolean
>;

export const agentComposerKeys = {
  all: ["agent-composer"] as const,
  entries: (
    projectId: string,
    conversationId: string | null | undefined,
    query: string,
  ) =>
    [
      ...agentComposerKeys.all,
      "entries",
      { projectId, conversationId: conversationId ?? null, query },
    ] as const,
  planReferences: (projectId: string, query: string) =>
    [
      ...agentComposerKeys.all,
      "plan-references",
      { projectId, query },
    ] as const,
  skills: (
    projectId: string,
    conversationId: string | null | undefined,
    providerHarness: string | null | undefined,
    mode: string | null | undefined,
  ) =>
    [
      ...agentComposerKeys.all,
      "skills",
      {
        projectId,
        conversationId: conversationId ?? null,
        providerHarness: providerHarness ?? null,
        mode: mode ?? null,
      },
    ] as const,
  integrations: (
    kind: AgentComposerIntegrationKind | null | undefined,
    query: string,
  ) =>
    [
      ...agentComposerKeys.all,
      "integrations",
      { kind: kind ?? null, query },
    ] as const,
};

export function useAgentComposerEntries({
  projectId,
  conversationId,
  query,
  enabled,
}: {
  projectId: string;
  conversationId?: string | null;
  query: string;
  enabled: boolean;
}) {
  const normalizedQuery = query.trim();
  return useQuery({
    queryKey: agentComposerKeys.entries(
      projectId,
      conversationId,
      normalizedQuery,
    ),
    queryFn: () =>
      agentComposerApi.searchEntries({
        projectId,
        query: normalizedQuery,
        limit: 80,
        ...(conversationId !== undefined ? { conversationId } : {}),
      }),
    enabled: enabled && projectId.length > 0,
    staleTime: 15_000,
    gcTime: 60_000,
    placeholderData: {
      entries: [] satisfies AgentComposerEntry[],
      truncated: false,
    },
  });
}

export function useAgentComposerPlanReferences({
  projectId,
  query,
  enabled,
}: {
  projectId: string;
  query: string;
  enabled: boolean;
}) {
  const normalizedQuery = query.trim();
  return useQuery({
    queryKey: agentComposerKeys.planReferences(projectId, normalizedQuery),
    queryFn: () =>
      agentComposerApi.searchPlanReferences({
        projectId,
        query: normalizedQuery,
        limit: 12,
      }),
    enabled: enabled && projectId.length > 0,
    staleTime: 10_000,
    gcTime: 60_000,
    placeholderData: {
      plans: [] satisfies AgentComposerPlanReference[],
      truncated: false,
    },
  });
}

export function useAgentComposerSkills({
  projectId,
  conversationId,
  providerHarness,
  mode,
  enabled,
}: {
  projectId: string;
  conversationId?: string | null;
  providerHarness?: string | null;
  mode?: string | null;
  enabled: boolean;
}) {
  return useQuery({
    queryKey: agentComposerKeys.skills(
      projectId,
      conversationId,
      providerHarness,
      mode,
    ),
    queryFn: () =>
      agentComposerApi.listSkills({
        projectId,
        ...(conversationId !== undefined ? { conversationId } : {}),
        ...(providerHarness !== undefined ? { providerHarness } : {}),
        ...(mode !== undefined ? { mode } : {}),
      }),
    enabled: enabled && projectId.length > 0,
    staleTime: 30_000,
    gcTime: 120_000,
    placeholderData: { skills: [] satisfies AgentComposerSkill[] },
  });
}

export function useAgentComposerIntegrationAvailability({
  projectId,
  enabled,
}: {
  projectId?: string | null;
  enabled: boolean;
}): AgentComposerIntegrationAvailability {
  const ticketingProviders = useTicketingProviders(projectId ?? undefined, {
    enabled,
  });
  const atlassianSettings = useQuery({
    queryKey: atlassianIntegrationKeys.settings(),
    queryFn: () => atlassianApi.getSettings(),
    enabled,
    staleTime: INTEGRATION_SETTINGS_STALE_TIME_MS,
  });
  const granolaSettings = useQuery({
    queryKey: granolaIntegrationKeys.settings(),
    queryFn: () => granolaApi.getSettings(),
    enabled,
    staleTime: INTEGRATION_SETTINGS_STALE_TIME_MS,
  });
  const validTicketingProviders = new Set(
    getValidTicketingProviders(ticketingProviders.data).map(
      ({ provider }) => provider,
    ),
  );

  return {
    jira: validTicketingProviders.has("jira"),
    confluence: isConfluenceConnected(atlassianSettings.data),
    linear: validTicketingProviders.has("linear"),
    clickup: validTicketingProviders.has("clickup"),
    granola: isGranolaConnected(granolaSettings.data),
  };
}

export function useAgentComposerIntegrationResources({
  kind,
  query,
  enabled,
}: {
  kind?: AgentComposerIntegrationKind | null;
  query: string;
  enabled: boolean;
}) {
  const normalizedQuery = query.trim();
  const debouncedClickUpQuery = useDebouncedString(
    normalizedQuery,
    kind === "clickup" ? CLICKUP_TASK_SEARCH_DEBOUNCE_MS : 0,
  );
  const effectiveQuery =
    kind === "clickup" ? debouncedClickUpQuery : normalizedQuery;
  return useQuery({
    queryKey: agentComposerKeys.integrations(kind, effectiveQuery),
    queryFn: async (): Promise<
      Array<
        | AtlassianResourceSummary
        | LinearIssueSummary
        | ClickUpTaskSummary
        | GranolaNoteSummary
      >
    > => {
      if (kind === "linear") {
        return linearApi.searchIssues({
          query: effectiveQuery,
          limit: 12,
        });
      }
      if (kind === "clickup") {
        return clickupApi.searchTasks({
          query: effectiveQuery,
          limit: CLICKUP_TASK_SEARCH_LIMIT,
        });
      }
      if (kind === "granola") {
        const page = await granolaApi.listNotes({ pageSize: 30 });
        const query = effectiveQuery.toLowerCase();
        if (!query) {
          return page.notes;
        }
        return page.notes.filter((note) =>
          [note.title, note.summary, note.id]
            .filter(Boolean)
            .some((value) => value!.toLowerCase().includes(query)),
        );
      }
      return atlassianApi.searchResources({
        kind: (kind ?? "jira") as AtlassianResourceKind,
        query: effectiveQuery,
        limit: 12,
      });
    },
    enabled:
      enabled &&
      kind !== null &&
      kind !== undefined &&
      (kind !== "clickup" || effectiveQuery === normalizedQuery),
    staleTime: 10_000,
    gcTime: 60_000,
    placeholderData: [] satisfies AtlassianResourceSummary[],
  });
}

function useDebouncedString(value: string, delayMs: number): string {
  const [debouncedValue, setDebouncedValue] = useState(() =>
    delayMs > 0 && value.length > 0 ? "" : value,
  );

  useEffect(() => {
    if (delayMs <= 0) {
      setDebouncedValue(value);
      return;
    }
    if (value.length === 0) {
      setDebouncedValue("");
      return;
    }

    const timer = window.setTimeout(() => setDebouncedValue(value), delayMs);
    return () => window.clearTimeout(timer);
  }, [delayMs, value]);

  return debouncedValue;
}
