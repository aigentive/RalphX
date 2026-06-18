import { useQuery } from "@tanstack/react-query";

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
import { linearApi, type LinearIssueSummary } from "@/api/linear";
import type { AgentComposerIntegrationKind } from "@/components/agents/composer/agentComposerCore";

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
  return useQuery({
    queryKey: agentComposerKeys.integrations(kind, normalizedQuery),
    queryFn: async (): Promise<
      Array<AtlassianResourceSummary | LinearIssueSummary>
    > => {
      if (kind === "linear") {
        return linearApi.searchIssues({
          query: normalizedQuery,
          limit: 12,
        });
      }
      return atlassianApi.searchResources({
        kind: (kind ?? "jira") as AtlassianResourceKind,
        query: normalizedQuery,
        limit: 12,
      });
    },
    enabled: enabled && kind !== null && kind !== undefined,
    staleTime: 10_000,
    gcTime: 60_000,
    placeholderData: [] satisfies AtlassianResourceSummary[],
  });
}
