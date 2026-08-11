import { ExternalLink, GitPullRequestArrow } from "lucide-react";

import type { AgentConversationWorkspace } from "@/api/chat";
import { PrStatusBadge } from "@/components/tasks/detail-views/shared/PrStatusBadge";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { useAfterPaint } from "@/components/ticketing/useAfterPaint";
import { usePullRequestDetail } from "@/hooks/usePullRequestDetail";

import {
  pullRequestSelectorFromShell,
  pullRequestShellFromWorkspace,
} from "@/components/pr/PullRequestDetailShell";
import { normalizePrStatus } from "@/components/pr/PullRequestDetailUtils";
import { PullRequestStatusStrip } from "@/components/pr/PullRequestStatusStrip";

import {
  blocksAgentWorkspaceGitInspection,
  canInspectAgentWorkspaceBaseFreshness,
  getAgentWorkspaceEffectiveBaseLabel,
  getAgentWorkspaceMaintenancePresentation,
  getAgentWorkspacePrConflictSummary,
  getAgentWorkspaceTerminalPublicationStatus,
  isPipelineOwnedAgentWorkspace,
} from "./agentWorkspacePublishState";
import {
  AgentWorkspaceBranchIdentity,
  AgentWorkspaceModeStatus,
  AgentWorkspaceSyncStatus,
} from "./AgentWorkspaceToolbarPresentation";
import { useAgentWorkspaceFullFreshness } from "./useAgentWorkspaceFullFreshness";
import { useDeferredAgentHydration } from "./useDeferredAgentHydration";

export type AgentWorkspaceToolbarResolutionState =
  "resolved" | "loading" | "unavailable";

function titleCaseStatus(value: string): string {
  const normalized = value.replace(/_/g, " ");
  return normalized.charAt(0).toUpperCase() + normalized.slice(1);
}

function workspaceModeLabel(workspace: AgentConversationWorkspace): string {
  if (workspace.mode === "edit") return "Edit";
  if (isPipelineOwnedAgentWorkspace(workspace)) return "Plan";
  if (workspace.mode === "review_pr") return "Review PR";
  return titleCaseStatus(workspace.mode);
}

function workspaceSyncLabel({
  workspace,
  maintenancePresentation,
  isRepairPending,
  hasPrConflict,
  baseBlocked,
  isBehindBase,
  terminalPrStatus,
}: {
  workspace: AgentConversationWorkspace;
  maintenancePresentation: ReturnType<typeof getAgentWorkspaceMaintenancePresentation>;
  isRepairPending: boolean;
  hasPrConflict: boolean;
  baseBlocked: boolean;
  isBehindBase: boolean;
  terminalPrStatus: "merged" | "closed" | null;
}): string | null {
  let value: string | null | undefined;
  if (!terminalPrStatus && maintenancePresentation) value = maintenancePresentation.title;
  else if (!terminalPrStatus && isRepairPending) value = "Repair pending";
  else if (!terminalPrStatus && hasPrConflict) value = "Conflicting";
  else if (!terminalPrStatus && baseBlocked) value = "Base unavailable";
  else if (!terminalPrStatus && isBehindBase) value = "Behind base";
  else value = workspace.publicationPushStatus ?? workspace.status;

  if (!value) return null;
  const label = titleCaseStatus(value);
  return terminalPrStatus && label.toLowerCase() === terminalPrStatus
    ? null
    : label;
}

function selectorKey(
  selector: ReturnType<typeof pullRequestSelectorFromShell>,
): string | null {
  if (!selector) return null;
  return [
    selector.projectId,
    selector.prNumber ?? "",
    selector.branch ?? "",
  ].join(":");
}

interface WorkspaceSyncState {
  workspace: AgentConversationWorkspace;
  maintenancePresentation: ReturnType<typeof getAgentWorkspaceMaintenancePresentation>;
  isRepairPending: boolean;
  hasPrConflict: boolean;
  baseBlocked: boolean;
  isBehindBase: boolean;
  terminalPrStatus: "merged" | "closed" | null;
}

function AgentWorkspacePrHealth({
  selector,
  shellStatus,
  syncState,
}: {
  selector: NonNullable<ReturnType<typeof pullRequestSelectorFromShell>>;
  shellStatus: ReturnType<typeof normalizePrStatus>;
  syncState: WorkspaceSyncState;
}) {
  const shouldFetchDetail = useAfterPaint(true);
  const detailQuery = usePullRequestDetail(selector, {
    enabled: shouldFetchDetail,
  });
  const detail = detailQuery.data;
  const detailStatus = normalizePrStatus(
    detail?.description?.state ?? null,
    detail?.description?.isDraft,
  );
  const detailTerminalPrStatus =
    detailStatus === "Merged"
      ? "merged"
      : detailStatus === "Closed"
        ? "closed"
        : null;
  const syncLabel = workspaceSyncLabel({
    ...syncState,
    terminalPrStatus: detailTerminalPrStatus ?? syncState.terminalPrStatus,
  });

  if (detailQuery.isError) {
    return (
      <>
        <AgentWorkspaceSyncStatus label={syncLabel} />
        <span
          className="shrink-0 rounded-full px-2 py-0.5 text-[0.6875rem] font-medium"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-muted)",
          }}
          role="status"
        >
          PR health unavailable
        </span>
      </>
    );
  }

  if (!detail) {
    return (
      <>
        <AgentWorkspaceSyncStatus label={syncLabel} />
        <PullRequestStatusStrip
          reviewSummary={null}
          checks={[]}
          loading
          variant="compact"
        />
      </>
    );
  }

  return (
    <>
      {detailStatus && detailStatus !== shellStatus ? (
        <PrStatusBadge status={detailStatus} />
      ) : null}
      <AgentWorkspaceSyncStatus label={syncLabel} />
      <PullRequestStatusStrip
        reviewSummary={detail.reviewSummary}
        checks={detail.checks}
        checksUnavailable={detail.sourcesUnavailable.includes("checks")}
        variant="compact"
      />
    </>
  );
}

export function AgentWorkspaceToolbar({
  workspace,
  resolutionState = "resolved",
}: {
  workspace: AgentConversationWorkspace | null;
  resolutionState?: AgentWorkspaceToolbarResolutionState | undefined;
}) {
  const canHydrateWorkspace =
    resolutionState === "resolved" && Boolean(workspace);
  const shouldHydrateFullFreshness = useDeferredAgentHydration(
    canHydrateWorkspace ? workspace?.conversationId : null,
  );
  const terminalPrStatus =
    getAgentWorkspaceTerminalPublicationStatus(workspace);
  const maintenancePresentation = getAgentWorkspaceMaintenancePresentation(workspace);
  const blocksGitInspection = blocksAgentWorkspaceGitInspection(workspace);
  const isRepairPending = Boolean(
    !workspace?.maintenanceOperation &&
      workspace?.publicationPushStatus === "needs_agent" &&
      !terminalPrStatus,
  );
  const canInspectBaseFreshness = canInspectAgentWorkspaceBaseFreshness(workspace);
  const fullFreshnessQuery = useAgentWorkspaceFullFreshness(
    workspace?.conversationId ?? null,
    {
      enabled: Boolean(
        shouldHydrateFullFreshness &&
        workspace &&
        !blocksGitInspection &&
        !isRepairPending &&
        canInspectBaseFreshness &&
        !terminalPrStatus,
      ),
    },
  );

  if (!workspace) {
    if (resolutionState === "loading") {
      return (
        <div
          className="flex shrink-0 items-center gap-2 px-4 py-2 text-xs"
          data-testid="agents-workspace-toolbar"
          role="status"
          aria-label="Loading workspace status"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderBottomColor: "var(--border-subtle)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
            color: "var(--text-muted)",
          }}
        >
          <span className="h-4 w-44 animate-pulse rounded-full bg-[var(--bg-hover)]" />
          Loading workspace status…
        </div>
      );
    }
    if (resolutionState === "unavailable") {
      return (
        <div
          className="shrink-0 px-4 py-2 text-xs"
          data-testid="agents-workspace-toolbar"
          role="status"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderBottomColor: "var(--border-subtle)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
            color: "var(--text-muted)",
          }}
        >
          Workspace status unavailable
        </div>
      );
    }
    return null;
  }

  const freshness =
    canInspectBaseFreshness &&
    fullFreshnessQuery.data?.conversationId === workspace.conversationId
      ? fullFreshnessQuery.data
      : undefined;
  const baseLabel = getAgentWorkspaceEffectiveBaseLabel(workspace, freshness);
  const shell = pullRequestShellFromWorkspace(workspace);
  const selector = pullRequestSelectorFromShell(shell);
  const prHealthKey = selectorKey(selector);
  const shellStatus = normalizePrStatus(shell?.status ?? null);
  const hasPrConflict = getAgentWorkspacePrConflictSummary(workspace) !== null;
  const baseBlocked = freshness?.baseStatus === "blocked";
  const isBehindBase = Boolean(
    freshness && !baseBlocked && freshness.isBaseAhead,
  );
  const syncState: WorkspaceSyncState = {
    workspace,
    maintenancePresentation,
    isRepairPending,
    hasPrConflict,
    baseBlocked,
    isBehindBase,
    terminalPrStatus,
  };
  const syncLabel = workspaceSyncLabel(syncState);

  return (
    <section
      className="@container shrink-0 px-4 py-2"
      data-testid="agents-workspace-toolbar"
      role="region"
      aria-label="Workspace status"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderBottomColor: "var(--border-subtle)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
      }}
    >
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5 text-xs">
        <AgentWorkspaceBranchIdentity
          branchName={workspace.branchName}
          baseLabel={baseLabel}
        />

        <span className="flex min-w-0 shrink-0 items-center gap-1.5 text-[var(--text-secondary)]">
          <GitPullRequestArrow
            className="h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]"
            aria-hidden="true"
          />
          {shell ? (
            shell.url ? (
              <button
                type="button"
                className="inline-flex min-w-0 items-center gap-1 rounded bg-transparent p-0 text-left font-medium text-[var(--text-primary)] transition-colors hover:text-[var(--accent-primary)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                onClick={() => void openExternalTicketUrl(shell.url!)}
                aria-label={
                  shell.prNumber
                    ? `Open PR #${shell.prNumber} in GitHub`
                    : "Open pull request in GitHub"
                }
              >
                <span className="truncate">
                  {shell.prNumber ? `PR #${shell.prNumber}` : "Pull request"}
                </span>
                <ExternalLink
                  className="h-3 w-3 shrink-0 text-[var(--text-muted)]"
                  aria-hidden="true"
                />
              </button>
            ) : (
              <span className="font-medium text-[var(--text-primary)]">
                {shell.prNumber ? `PR #${shell.prNumber}` : "Pull request"}
              </span>
            )
          ) : (
            <span className="font-medium text-[var(--text-primary)]">
              No PR yet
            </span>
          )}
        </span>

        {shellStatus ? <PrStatusBadge status={shellStatus} /> : null}

        {!selector ? <AgentWorkspaceSyncStatus label={syncLabel} /> : null}

        {selector && prHealthKey ? (
          <div
            className="flex min-w-0 flex-wrap items-center gap-2"
            data-testid="agents-workspace-pr-health"
            aria-live="polite"
          >
            <AgentWorkspacePrHealth
              key={prHealthKey}
              selector={selector}
              shellStatus={shellStatus}
              syncState={syncState}
            />
          </div>
        ) : null}

        <AgentWorkspaceModeStatus label={workspaceModeLabel(workspace)} />
      </div>
    </section>
  );
}
