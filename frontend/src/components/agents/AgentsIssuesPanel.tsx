import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertCircle,
  CheckCircle2,
  GitBranchPlus,
  Loader2,
  XCircle,
} from "lucide-react";
import { useMemo } from "react";
import { toast } from "sonner";

import {
  chatApi,
  type AgentConversationIssue,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

import { EmptyArtifactState } from "./AgentsArtifactEmptyState";

interface AgentsIssuesPanelProps {
  conversationId: string | null;
  projectId: string | null;
}

const agentConversationIssueKeys = {
  list: (conversationId: string | null) =>
    ["agents", "conversation-issues", conversationId, "open"] as const,
};

const severityTone: Record<string, string> = {
  critical: "var(--status-error)",
  high: "var(--status-error)",
  medium: "var(--status-warning)",
  low: "var(--text-muted)",
  info: "var(--accent-primary)",
};

export function AgentsIssuesPanel({
  conversationId,
  projectId: _projectId,
}: AgentsIssuesPanelProps) {
  const queryClient = useQueryClient();
  const issuesQuery = useQuery({
    queryKey: agentConversationIssueKeys.list(conversationId),
    queryFn: () => chatApi.listAgentConversationIssues(conversationId!),
    enabled: Boolean(conversationId),
    staleTime: 5_000,
  });
  const issues = issuesQuery.data ?? [];
  const issueCountLabel = useMemo(() => {
    if (issues.length === 0) return "No open issues";
    if (issues.length === 1) return "1 open issue";
    return `${issues.length} open issues`;
  }, [issues.length]);

  const invalidateIssues = () =>
    queryClient.invalidateQueries({
      queryKey: agentConversationIssueKeys.list(conversationId),
    });

  const convertMutation = useMutation({
    mutationFn: (issueId: string) =>
      chatApi.convertAgentConversationIssueFollowup(issueId),
    onSuccess: (issue) => {
      void invalidateIssues();
      toast.success(
        issue.linkedFollowupConversationId
          ? "Follow-up Agent conversation ready"
          : "Issue follow-up created",
      );
    },
    onError: (err) => {
      toast.error(
        err instanceof Error
          ? err.message
          : "Failed to create follow-up Agent conversation",
      );
    },
  });

  const statusMutation = useMutation({
    mutationFn: ({
      issueId,
      status,
    }: {
      issueId: string;
      status: "resolved" | "dismissed";
    }) => chatApi.updateAgentConversationIssueStatus(issueId, status),
    onSuccess: (_issue, variables) => {
      void invalidateIssues();
      toast.success(variables.status === "resolved" ? "Issue resolved" : "Issue dismissed");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to update issue");
    },
  });

  const isMutating = convertMutation.isPending || statusMutation.isPending;

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{
        backgroundColor: "var(--bg-base)",
        color: "var(--text-primary)",
      }}
    >
      <div
        className="flex items-center justify-between gap-3 border-b px-4 py-3"
        style={{
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: 0,
          borderBottomWidth: 1,
        }}
      >
        <div className="flex min-w-0 items-center gap-2">
          <AlertCircle
            className="h-4 w-4 shrink-0"
            style={{ color: "var(--status-warning)" }}
          />
          <div className="min-w-0">
            <h2 className="truncate text-sm font-semibold">Issues</h2>
            <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
              {issueCountLabel}
            </p>
          </div>
        </div>
        {issuesQuery.isFetching ? (
          <Loader2
            className="h-4 w-4 animate-spin"
            style={{ color: "var(--text-muted)" }}
          />
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {!conversationId ? (
          <EmptyArtifactState title="No conversation selected" />
        ) : issuesQuery.isLoading ? (
          <EmptyArtifactState title="Loading issues..." />
        ) : issues.length === 0 ? (
          <EmptyArtifactState
            title="No open issues"
            detail="Agent drift, blockers, and decisions that need user attention will appear here."
          />
        ) : (
          <div className="space-y-3">
            {issues.map((issue) => (
              <IssueCard
                key={issue.id}
                issue={issue}
                disabled={isMutating}
                onCreateFollowup={() => convertMutation.mutate(issue.id)}
                onResolve={() =>
                  statusMutation.mutate({ issueId: issue.id, status: "resolved" })
                }
                onDismiss={() =>
                  statusMutation.mutate({ issueId: issue.id, status: "dismissed" })
                }
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function IssueCard({
  issue,
  disabled,
  onCreateFollowup,
  onResolve,
  onDismiss,
}: {
  issue: AgentConversationIssue;
  disabled: boolean;
  onCreateFollowup: () => void;
  onResolve: () => void;
  onDismiss: () => void;
}) {
  const severityColor = severityTone[issue.severity.toLowerCase()] ?? "var(--text-muted)";
  return (
    <article
      className="rounded-md border p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: 1,
      }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <span
              className="rounded px-1.5 py-0.5 text-[0.6875rem] font-semibold uppercase tracking-normal"
              style={{
                backgroundColor: "var(--overlay-faint)",
                color: severityColor,
              }}
            >
              {formatLabel(issue.severity)}
            </span>
            <span
              className="rounded px-1.5 py-0.5 text-[0.6875rem] font-medium"
              style={{
                backgroundColor: "var(--overlay-faint)",
                color: "var(--text-muted)",
              }}
            >
              {formatLabel(issue.issueKind)}
            </span>
            <span
              className="rounded px-1.5 py-0.5 text-[0.6875rem] font-medium"
              style={{
                backgroundColor: "var(--overlay-faint)",
                color: "var(--text-muted)",
              }}
            >
              {formatLabel(issue.blockingScope)}
            </span>
          </div>
          <div>
            <h3 className="text-sm font-semibold leading-snug">{issue.title}</h3>
            <p
              className="mt-1 whitespace-pre-wrap text-xs leading-relaxed"
              style={{ color: "var(--text-secondary)" }}
            >
              {issue.summary}
            </p>
          </div>
        </div>
      </div>

      <dl className="mt-3 grid grid-cols-1 gap-2 text-xs sm:grid-cols-2">
        {issue.sourceAgentName ? (
          <IssueMeta label="Agent" value={issue.sourceAgentName} />
        ) : null}
        {issue.sourceTaskId ? (
          <IssueMeta label="Task" value={issue.sourceTaskId} monospace />
        ) : null}
        {issue.blockerFingerprint ? (
          <IssueMeta label="Fingerprint" value={issue.blockerFingerprint} monospace />
        ) : null}
        <IssueMeta label="Updated" value={formatDate(issue.updatedAt)} />
      </dl>

      {issue.evidence ? (
        <IssueSection label="Evidence" value={issue.evidence} />
      ) : null}
      {issue.recommendation ? (
        <IssueSection label="Recommendation" value={issue.recommendation} />
      ) : null}

      <div className="mt-4 flex flex-wrap items-center gap-2">
        {issue.linkedFollowupConversationId ? (
          <span
            className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs font-medium"
            style={{
              backgroundColor: "var(--accent-muted)",
              color: "var(--accent-primary)",
            }}
          >
            <GitBranchPlus className="h-3.5 w-3.5" />
            Follow-up created
          </span>
        ) : (
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={onCreateFollowup}
            disabled={disabled}
            className="h-8 gap-1.5"
          >
            <GitBranchPlus className="h-3.5 w-3.5" />
            Create follow-up
          </Button>
        )}
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={onResolve}
          disabled={disabled}
          className="h-8 gap-1.5"
        >
          <CheckCircle2 className="h-3.5 w-3.5" />
          Resolve
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={onDismiss}
          disabled={disabled}
          className="h-8 gap-1.5"
        >
          <XCircle className="h-3.5 w-3.5" />
          Dismiss
        </Button>
      </div>
    </article>
  );
}

function IssueMeta({
  label,
  value,
  monospace = false,
}: {
  label: string;
  value: string;
  monospace?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="font-medium" style={{ color: "var(--text-muted)" }}>
        {label}
      </dt>
      <dd
        className={cn("truncate", monospace && "font-mono")}
        style={{ color: "var(--text-secondary)" }}
      >
        {value}
      </dd>
    </div>
  );
}

function IssueSection({ label, value }: { label: string; value: string }) {
  return (
    <section className="mt-3">
      <h4 className="text-xs font-semibold" style={{ color: "var(--text-muted)" }}>
        {label}
      </h4>
      <p
        className="mt-1 whitespace-pre-wrap text-xs leading-relaxed"
        style={{ color: "var(--text-secondary)" }}
      >
        {value}
      </p>
    </section>
  );
}

function formatLabel(value: string): string {
  return value
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}
