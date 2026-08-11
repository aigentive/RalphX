import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, GitBranch, GitPullRequestArrow } from "lucide-react";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import { formatBranchDisplay } from "@/lib/branch-utils";

import { formatPullRequestUrlLabel } from "./agentPublishFormatting";
import {
  getAgentWorkspaceTerminalPublicationLabel,
  getAgentWorkspaceTerminalPublicationStatus,
} from "./agentWorkspacePublishState";

function workspaceStatusLabel(
  workspace: AgentConversationWorkspace,
  freshness: AgentConversationWorkspaceFreshness | undefined,
) {
  const terminalStatus = getAgentWorkspaceTerminalPublicationStatus(workspace);
  const isBaseBlocked = freshness?.baseStatus === "blocked";
  const isBehindBase =
    !isBaseBlocked && !terminalStatus && Boolean(freshness?.isBaseAhead);

  return {
    label: terminalStatus
      ? (getAgentWorkspaceTerminalPublicationLabel(workspace) ?? "")
      : isBaseBlocked
        ? "Base unavailable"
        : isBehindBase
          ? "Behind base"
          : (workspace.publicationPushStatus ?? workspace.status).replace(
              /_/g,
              " ",
            ),
    isWarning: isBaseBlocked || isBehindBase,
  };
}

export function AgentConversationWorkspaceLine({
  freshness,
  workspace,
}: {
  freshness?: AgentConversationWorkspaceFreshness;
  workspace: AgentConversationWorkspace | null;
}) {
  if (!workspace) {
    return null;
  }

  const branch = formatBranchDisplay(workspace.branchName);
  const status = workspaceStatusLabel(workspace, freshness);
  const statusColor = status.isWarning
    ? "var(--status-warning)"
    : "var(--text-secondary)";
  const prLabel =
    workspace.publicationPrNumber != null
      ? `PR #${workspace.publicationPrNumber}`
      : workspace.publicationPrUrl
        ? "Published PR"
        : null;
  const prUrlLabel = workspace.publicationPrUrl
    ? formatPullRequestUrlLabel(workspace.publicationPrUrl)
    : null;

  return (
    <div
      className="flex min-w-0 flex-wrap items-center justify-end gap-x-2 gap-y-1 text-[0.75rem] font-medium"
      data-testid="agents-conversation-workspace-line"
      aria-label={`Workspace branch ${branch.full}, ${status.label}`}
    >
      <span
        className="inline-flex min-w-0 items-center gap-1.5"
        style={{ color: statusColor }}
        data-testid="agents-conversation-branch-line"
      >
        <GitBranch
          className="h-3.5 w-3.5 shrink-0"
          style={{ color: "var(--text-muted)" }}
          aria-hidden="true"
        />
        <span className="min-w-0 truncate font-mono">{branch.short}</span>
        <span
          className="h-1 w-1 shrink-0 rounded-full"
          style={{
            backgroundColor: status.isWarning
              ? "var(--status-warning)"
              : "var(--accent-primary)",
          }}
          aria-hidden="true"
        />
        <span className="shrink-0 capitalize">{status.label}</span>
      </span>

      {prLabel ? (
        <span
          className="inline-flex min-w-0 items-center gap-1.5"
          style={{ color: "var(--text-secondary)" }}
        >
          <GitPullRequestArrow
            className="h-3.5 w-3.5 shrink-0"
            style={{ color: "var(--text-muted)" }}
            aria-hidden="true"
          />
          {workspace.publicationPrUrl ? (
            <button
              type="button"
              className="inline-flex min-w-0 items-center gap-1 truncate bg-transparent p-0 text-left font-medium text-[var(--text-primary)] transition-colors hover:text-[var(--accent-primary)]"
              onClick={() => void openUrl(workspace.publicationPrUrl!)}
              data-testid="agents-conversation-pr-link"
              aria-label={`Open ${prUrlLabel ?? prLabel} in browser`}
            >
              {prLabel}
              <ExternalLink
                className="h-3 w-3 shrink-0"
                style={{ color: "var(--text-muted)" }}
                aria-hidden="true"
              />
            </button>
          ) : (
            <span
              className="truncate font-medium text-[var(--text-primary)]"
              data-testid="agents-conversation-pr-label"
            >
              {prLabel}
            </span>
          )}
        </span>
      ) : null}
    </div>
  );
}
