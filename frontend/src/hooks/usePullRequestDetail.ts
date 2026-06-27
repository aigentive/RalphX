import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "@/components/agents/agentWorkspaceQueries";

export type PullRequestDetailStatus = "Draft" | "Open" | "Merged" | "Closed";
export type PullRequestDetailOrigin = "publication" | "source";

export interface PullRequestDetailViewModel {
  origin: PullRequestDetailOrigin;
  number: number | null;
  status: PullRequestDetailStatus | null;
  url: string | null;
  headRef: string | null;
  baseRef: string | null;
  pushStatus: string | null;
  supervisionStatus: string | null;
  supervisionSummary: string | null;
  supervisionUpdatedAt: string | null;
  title?: string;
}

const STATUS_BY_NORMALIZED_VALUE: Record<string, PullRequestDetailStatus> = {
  closed: "Closed",
  draft: "Draft",
  merged: "Merged",
  open: "Open",
};

function nonEmptyString(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

export function normalizePrStatus(
  raw: string | null,
): PullRequestDetailStatus | null {
  const normalized = nonEmptyString(raw)?.toLowerCase();
  return normalized ? STATUS_BY_NORMALIZED_VALUE[normalized] ?? null : null;
}

export function mapPullRequestDetail(
  workspace: AgentConversationWorkspace | null | undefined,
): PullRequestDetailViewModel | null {
  if (!workspace) {
    return null;
  }

  const sourcePullRequest = workspace.sourcePullRequest ?? null;
  const publicationUrl = nonEmptyString(workspace.publicationPrUrl);
  const sourceUrl = nonEmptyString(sourcePullRequest?.url);
  const hasPublicationPr =
    workspace.publicationPrNumber !== null || publicationUrl !== null;

  if (!hasPublicationPr && !sourcePullRequest) {
    return null;
  }

  const title = nonEmptyString(sourcePullRequest?.title);
  const detail: PullRequestDetailViewModel = {
    origin: hasPublicationPr ? "publication" : "source",
    number: workspace.publicationPrNumber ?? sourcePullRequest?.number ?? null,
    status: normalizePrStatus(workspace.publicationPrStatus),
    url: publicationUrl ?? sourceUrl,
    headRef: nonEmptyString(sourcePullRequest?.headRefName),
    baseRef:
      nonEmptyString(workspace.baseRef) ??
      nonEmptyString(sourcePullRequest?.baseRefName),
    pushStatus: nonEmptyString(workspace.publicationPushStatus),
    supervisionStatus: nonEmptyString(workspace.prSupervisionStatus),
    supervisionSummary: nonEmptyString(workspace.prSupervisionSummary),
    supervisionUpdatedAt: nonEmptyString(workspace.prSupervisionUpdatedAt),
  };

  if (title) {
    detail.title = title;
  }

  return detail;
}

export function usePullRequestDetail(conversationId: string | null | undefined) {
  return useQuery({
    queryKey: agentWorkspaceKeys.workspace(conversationId),
    queryFn: () => chatApi.getAgentConversationWorkspace(conversationId!),
    enabled: Boolean(conversationId),
    staleTime: AGENT_WORKSPACE_STALE_MS,
    select: mapPullRequestDetail,
  });
}
