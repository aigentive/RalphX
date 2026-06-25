import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, GitBranch, Loader2, Wrench } from "lucide-react";

import { diffApi } from "@/api/diff";
import type { AgentWorkspaceChangeSummary } from "@/api/diff";
import type { AgentConversationWorkspace } from "@/api/chat";

import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";

function formatBucket(
  summary: AgentWorkspaceChangeSummary | undefined,
  bucket: "staged" | "unstaged",
) {
  const data = summary?.[bucket];
  if (!data) return "Loading";
  const fileLabel = data.fileCount === 1 ? "file" : "files";
  return `${data.fileCount} ${fileLabel}, +${data.additions} -${data.deletions}`;
}

function repairStateLabel(summary: AgentWorkspaceChangeSummary | undefined) {
  const state = summary?.repairState;
  if (!state) return "Inspecting repair state";
  if (state.rebaseInProgress) return "Rebase paused for repair";
  if (state.mergeInProgress) return "Merge paused for repair";
  if (state.checkedOutBranch === state.expectedBranch) return "Branch ready for repair";
  return "Repair state detected";
}

export function AgentsPublishRepairState({
  workspace,
  base,
  canHydratePublishFacts,
  focusRequest,
}: {
  workspace: AgentConversationWorkspace;
  base: string;
  canHydratePublishFacts: boolean;
  focusRequest?: AgentPublishFocusRequest | null | undefined;
}) {
  const conversationId = workspace.conversationId;
  const repairSummaryQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "repair-summary"],
    queryFn: () =>
      diffApi.getAgentConversationWorkspaceRepairChangeSummary(conversationId),
    enabled: canHydratePublishFacts,
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval: 3_000,
  });
  const summary = repairSummaryQuery.data;
  const conflicted = summary?.conflicted;
  const conflictedFiles = conflicted?.files ?? [];
  const conflictedCount = conflicted?.fileCount ?? 0;
  const technicalError = repairSummaryQuery.error;

  return (
    <section
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border"
      data-testid="agents-publish-repair-state"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div
        className="border-b px-3 py-3"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "0 0 1px 0",
        }}
      >
        <div className="flex items-start gap-2">
          <span
            className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full"
            style={{
              backgroundColor: "var(--status-warning-muted)",
              color: "var(--status-warning)",
            }}
          >
            <Wrench className="h-3.5 w-3.5" aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <div
              className="text-sm font-semibold"
              style={{ color: "var(--text-primary)" }}
            >
              Repairing workspace
            </div>
            <p
              className="mt-1 text-xs leading-relaxed"
              style={{ color: "var(--text-secondary)" }}
            >
              RalphX routed this workspace to the agent for repair. Publishing controls
              are paused until the repair completes.
            </p>
          </div>
        </div>

        <div
          className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs"
          style={{ color: "var(--text-secondary)" }}
        >
          <span className="inline-flex min-w-0 items-center gap-1.5">
            <GitBranch
              className="h-3 w-3 shrink-0"
              style={{ color: "var(--text-muted)" }}
              aria-hidden="true"
            />
            <span
              className="truncate font-medium"
              style={{ color: "var(--text-primary)" }}
            >
              {workspace.branchName}
            </span>
            <span style={{ color: "var(--text-muted)" }} aria-hidden="true">
              &rarr;
            </span>
            <span className="truncate" style={{ color: "var(--text-muted)" }}>
              {base}
            </span>
          </span>
          <span
            className="hidden @xs:inline-block"
            style={{ color: "var(--border-subtle)" }}
            aria-hidden="true"
          >
            |
          </span>
          <span data-testid="agents-publish-repair-state-label">
            {repairStateLabel(summary)}
          </span>
        </div>

        <div className="mt-3 flex flex-wrap gap-2 text-[0.6875rem]">
          <span
            className="rounded-full border px-2 py-1"
            data-testid="agents-publish-repair-bucket-conflicted"
            style={{
              backgroundColor:
                conflictedCount > 0 ? "var(--status-warning-muted)" : "var(--bg-subtle)",
              borderColor:
                conflictedCount > 0
                  ? "var(--status-warning-border)"
                  : "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
          >
            Conflicted: {conflictedCount}
          </span>
          <span
            className="rounded-full border px-2 py-1"
            data-testid="agents-publish-repair-bucket-unstaged"
            style={{
              backgroundColor: "var(--bg-subtle)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
          >
            Unstaged: {formatBucket(summary, "unstaged")}
          </span>
          <span
            className="rounded-full border px-2 py-1"
            data-testid="agents-publish-repair-bucket-staged"
            style={{
              backgroundColor: "var(--bg-subtle)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
          >
            Staged: {formatBucket(summary, "staged")}
          </span>
        </div>
      </div>

      {technicalError ? (
        <div
          className="m-3 rounded-md border px-3 py-2 text-xs"
          data-testid="agents-publish-repair-diff-error"
          style={{
            backgroundColor: "var(--bg-subtle)",
            borderColor: "var(--status-warning-border)",
            borderStyle: "solid",
            borderWidth: "1px",
            color: "var(--text-secondary)",
          }}
        >
          <div className="flex items-start gap-2">
            <AlertTriangle
              className="mt-0.5 h-3.5 w-3.5 shrink-0"
              style={{ color: "var(--status-warning)" }}
              aria-hidden="true"
            />
            <div>
              <div className="font-medium" style={{ color: "var(--text-primary)" }}>
                Repair diffs unavailable
              </div>
              <details
                className="mt-1"
                data-testid="agents-publish-repair-technical-details"
              >
                <summary className="cursor-pointer">Technical detail</summary>
                <p className="mt-1 font-mono text-[0.6875rem]">
                  {technicalError instanceof Error
                    ? technicalError.message
                    : String(technicalError)}
                </p>
              </details>
            </div>
          </div>
        </div>
      ) : conflictedFiles.length > 0 ? (
        <div
          className="border-b px-3 py-2 text-xs"
          data-testid="agents-publish-repair-conflicted-files"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "0 0 1px 0",
            color: "var(--text-secondary)",
          }}
        >
          <div className="font-medium" style={{ color: "var(--text-primary)" }}>
            Conflicted files
          </div>
          <div className="mt-1 flex flex-wrap gap-1.5">
            {conflictedFiles.map((file) => (
              <span
                key={file}
                className="rounded border px-1.5 py-0.5 font-mono text-[0.6875rem]"
                style={{
                  backgroundColor: "var(--bg-subtle)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: "var(--text-secondary)",
                }}
              >
                {file}
              </span>
            ))}
          </div>
        </div>
      ) : null}

      {summary ? (
        <AgentsPublishInlineDiffs
          conversationId={conversationId}
          review={null}
          commits={[]}
          isLoading={false}
          liveSummary={summary}
          repairMode
          defaultMode="uncommitted"
          workspaceChangeLabel="Repair changes"
          focusRequest={focusRequest}
        />
      ) : !technicalError ? (
        <div
          className="flex min-h-32 flex-1 items-center justify-center gap-2 text-xs"
          data-testid="agents-publish-repair-loading"
          style={{ color: "var(--text-muted)" }}
        >
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
          Loading repair changes
        </div>
      ) : null}
    </section>
  );
}
