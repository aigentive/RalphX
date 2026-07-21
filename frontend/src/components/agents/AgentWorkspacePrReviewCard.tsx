import { useMemo } from "react";
import {
  CheckCircle2,
  ExternalLink,
  GitPullRequestArrow,
  Loader2,
  MessageSquare,
  ShieldCheck,
  XCircle,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import type {
  AgentWorkspacePrReviewAction,
  AgentWorkspacePrReviewContext,
  AgentWorkspacePrReviewMonitorStatus,
} from "@/api/chat";
import { chatApi } from "@/api/chat";
import { Button } from "@/components/ui/button";

import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import {
  getAgentWorkspacePrReviewPresentation,
  lastResolvedPrReviewAction,
  prReviewActionCopy,
  prReviewStatusLabel,
} from "./agentWorkspacePrReviewPresentation";

interface AgentWorkspacePrReviewCardProps {
  conversationId: string;
  context: AgentWorkspacePrReviewContext | null | undefined;
  isLoading: boolean;
  isFetching: boolean;
  error: unknown;
}

const MONITOR_STATUS_LABELS: Record<AgentWorkspacePrReviewMonitorStatus, string> = {
  idle: "Idle",
  reviewing: "Reviewing",
  awaiting_user: "Awaiting user",
  watching: "Monitoring",
  submitting: "Submitting",
  blocked: "Blocked",
  paused: "Re-review paused",
  terminal: "Complete",
};

export function AgentWorkspacePrReviewCard({
  conversationId,
  context,
  isLoading,
  isFetching,
  error,
}: AgentWorkspacePrReviewCardProps) {
  const queryClient = useQueryClient();
  const resolvedAction = useMemo(
    () => lastResolvedPrReviewAction(context?.recentActions ?? []),
    [context?.recentActions],
  );
  const submitMutation = useMutation({
    mutationFn: (action: AgentWorkspacePrReviewAction) =>
      chatApi.submitAgentWorkspacePrReviewAction(
        conversationId,
        action.id,
        action.proposedAction,
      ),
    onSuccess: (result) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.prReview(conversationId),
        (previous: AgentWorkspacePrReviewContext | undefined) =>
          previous
            ? {
                ...previous,
                monitor: result.monitor,
                pendingAction: null,
                pendingActionHeadStatus: null,
                recentActions: [
                  result.action,
                  ...previous.recentActions.filter(
                    (action) => action.id !== result.action.id,
                  ),
                ],
              }
            : previous,
      );
      toast.success(
        prReviewActionCopy(result.action.proposedAction).submittedToast,
      );
      void invalidateWorkspaceQueries(queryClient, conversationId);
    },
    onError: (err) => {
      toast.error(
        err instanceof Error ? err.message : "Failed to submit PR review",
      );
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.prReview(conversationId),
      });
    },
  });

  const skipMutation = useMutation({
    mutationFn: (action: AgentWorkspacePrReviewAction) =>
      chatApi.skipAgentWorkspacePrReviewAction(
        conversationId,
        action.id,
        "Skipped from Review PR action card",
      ),
    onSuccess: (result) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.prReview(conversationId),
        (previous: AgentWorkspacePrReviewContext | undefined) =>
          previous
            ? {
                ...previous,
                monitor: result.monitor,
                pendingAction: null,
                pendingActionHeadStatus: null,
                recentActions: [
                  result.action,
                  ...previous.recentActions.filter(
                    (action) => action.id !== result.action.id,
                  ),
                ],
              }
            : previous,
      );
      toast.success("PR review action skipped");
      void invalidateWorkspaceQueries(queryClient, conversationId);
    },
    onError: (err) => {
      toast.error(
        err instanceof Error ? err.message : "Failed to skip PR review action",
      );
    },
  });

  if (isLoading && !context) {
    return (
      <div
        className="mx-2 mb-3 rounded-md border px-3 py-2.5 text-[0.8125rem]"
        style={{
          background: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          color: "var(--text-secondary)",
        }}
        data-testid="agent-workspace-pr-review-card-loading"
      >
        <div className="flex items-center gap-2">
          <Loader2 className="h-4 w-4 animate-spin" />
          <span>Loading PR review state...</span>
        </div>
      </div>
    );
  }

  if (error && !context) {
    return (
      <div
        className="mx-2 mb-3 rounded-md border px-3 py-2.5 text-[0.8125rem]"
        style={{
          background: "var(--bg-surface)",
          borderColor: "var(--status-error-muted)",
          color: "var(--text-secondary)",
        }}
        data-testid="agent-workspace-pr-review-card-error"
      >
        <div className="flex items-center gap-2">
          <XCircle className="h-4 w-4" style={{ color: "var(--status-error)" }} />
          <span>Review PR context is unavailable.</span>
        </div>
      </div>
    );
  }

  if (!context) {
    return null;
  }

  const presentation = getAgentWorkspacePrReviewPresentation(context);
  const pendingAction = presentation.pendingAction;
  const monitorStatus = presentation.isTerminal
    ? "terminal"
    : context.monitor?.status ?? null;
  const monitorLabel = monitorStatus
    ? MONITOR_STATUS_LABELS[monitorStatus] ?? prReviewStatusLabel(monitorStatus)
    : "Not started";
  const copy = pendingAction
    ? prReviewActionCopy(pendingAction.proposedAction)
    : null;
  const ActionIcon = pendingAction
    ? pendingAction.proposedAction === "approve"
      ? ShieldCheck
      : pendingAction.proposedAction === "comment"
        ? MessageSquare
        : XCircle
    : GitPullRequestArrow;
  const isMutating = submitMutation.isPending || skipMutation.isPending;
  const submitFailureMessage = pendingAction ? context.monitor?.lastError : null;
  return (
    <section
      className="mx-2 mb-3 overflow-hidden rounded-md border text-[0.8125rem]"
      style={{
        background: "var(--bg-surface)",
        borderColor: pendingAction
          ? "var(--accent-primary)"
          : "var(--border-subtle)",
        color: "var(--text-primary)",
      }}
      data-testid="agent-workspace-pr-review-card"
    >
      <div
        className="flex flex-wrap items-center gap-2 border-b px-3 py-2"
        style={{ borderColor: "var(--overlay-weak)" }}
      >
        <span
          className="flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-md"
          style={{
            background: pendingAction
              ? "var(--accent-muted)"
              : "var(--overlay-weak)",
            color: pendingAction
              ? "var(--accent-primary)"
              : "var(--text-secondary)",
          }}
        >
          <ActionIcon className="h-4 w-4" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[0.8125rem] font-semibold">
            {copy?.title ?? "Review PR monitor"}
          </div>
          <div
            className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[0.72rem]"
            style={{ color: "var(--text-muted)" }}
          >
            <span className="min-w-0 truncate">
              PR #{context.prNumber} · {presentation.headDetail}
            </span>
            {context.prUrl ? (
              <button
                type="button"
                className="inline-flex shrink-0 items-center gap-1 font-semibold hover:underline"
                style={{ color: "var(--accent-primary)" }}
                aria-label={`Open PR #${context.prNumber} in GitHub`}
                onClick={() => void openUrl(context.prUrl!)}
              >
                Open in GitHub
                <ExternalLink className="h-3 w-3" aria-hidden="true" />
              </button>
            ) : null}
            {isFetching ? <span className="shrink-0">refreshing</span> : null}
          </div>
        </div>
        <span
          className="rounded px-2 py-1 text-[0.6875rem] font-semibold"
          style={{
            background: pendingAction
              ? "var(--accent-muted)"
              : "var(--overlay-weak)",
            color: pendingAction
              ? "var(--accent-primary)"
              : "var(--text-muted)",
          }}
        >
          {pendingAction ? "Needs approval" : monitorLabel}
        </span>
      </div>

      <div className="space-y-2 px-3 py-2.5">
        {pendingAction ? (
          <>
            <p
              className="line-clamp-3 leading-relaxed"
              style={{ color: "var(--text-secondary)" }}
            >
              {pendingAction.summary}
            </p>
            <details>
              <summary
                className="cursor-pointer text-[0.75rem] font-semibold"
                style={{ color: "var(--accent-primary)" }}
              >
                Review body
              </summary>
              <pre
                className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-md border p-2 text-[0.75rem] leading-relaxed"
                style={{
                  background: "var(--bg-elevated)",
                  borderColor: "var(--border-subtle)",
                  color: "var(--text-secondary)",
                }}
              >
                {pendingAction.reviewBody}
              </pre>
            </details>
            {presentation.submitBlockedMessage ? (
              <div
                className="rounded-md border px-2.5 py-2 text-[0.75rem] leading-relaxed"
                style={{
                  background: "var(--status-warning-muted)",
                  borderColor: "var(--status-warning-border)",
                  color: "var(--text-secondary)",
                }}
              >
                {presentation.submitBlockedMessage}
              </div>
            ) : null}
            {submitFailureMessage ? (
              <div
                className="rounded-md border px-2.5 py-2 text-[0.75rem] leading-relaxed"
                style={{
                  background: "var(--status-warning-muted)",
                  borderColor: "var(--status-warning-border)",
                  color: "var(--text-secondary)",
                }}
              >
                Previous submit failed. {submitFailureMessage} You can retry
                from this card.
              </div>
            ) : null}
            <div className="flex flex-wrap items-center justify-end gap-2 pt-1">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => skipMutation.mutate(pendingAction)}
                disabled={isMutating}
              >
                Skip
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={() => submitMutation.mutate(pendingAction)}
                disabled={isMutating || !presentation.canSubmit}
              >
                {submitMutation.isPending ? (
                  <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                ) : (
                  <CheckCircle2 className="mr-1.5 h-3.5 w-3.5" />
                )}
                {copy?.primaryLabel}
              </Button>
            </div>
          </>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="min-w-0">
              <div
                className="text-[0.75rem] font-semibold"
                style={{ color: "var(--text-secondary)" }}
              >
                {resolvedAction
                  ? `Last action: ${prReviewStatusLabel(resolvedAction.status)}`
                  : presentation.consistencyMessage ??
                    "Waiting for a reviewer proposal"}
              </div>
              <div
                className="truncate text-[0.72rem]"
                style={{ color: "var(--text-muted)" }}
              >
                {context.monitor?.lastError ??
                  (presentation.isTerminal
                    ? "The remote pull request is merged or closed. Review history remains available."
                    : context.monitor?.monitorEnabled
                    ? "New PR commits can be reviewed again by the Review PR agent."
                    : "The Review PR agent will propose an action after it finishes reviewing.")}
              </div>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
