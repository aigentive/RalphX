import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, Loader2, RefreshCw, Search, Ticket, Unlink } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";

import {
  linearApi,
  type AgentConversationLinearIssue,
  type LinearIssueSummary,
} from "@/api/linear";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { agentLinearIssueKeys } from "./agentLinearIssueQueries";
import { ArtifactSelectableRegion } from "./artifact-selection/ArtifactSelectableRegion";

interface AgentsLinearIssuePanelProps {
  conversationId: string | null;
  projectId: string | null;
}

type RefreshLinearIssueOptions = {
  silent?: boolean;
};

export function AgentsLinearIssuePanel({
  conversationId,
  projectId,
}: AgentsLinearIssuePanelProps) {
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [isReassigning, setIsReassigning] = useState(false);
  const issueQuery = useQuery({
    queryKey: agentLinearIssueKeys.issue(conversationId),
    queryFn: () =>
      linearApi.getAgentConversationLinearIssue({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId),
    staleTime: 5_000,
  });
  const issue = issueQuery.data ?? null;
  const showSearch = !issue || isReassigning;
  const searchQuery = useQuery({
    queryKey: ["agents", "linear-issue-search", query.trim()] as const,
    queryFn: () => linearApi.searchIssues({ query: query.trim(), limit: 12 }),
    enabled: Boolean(conversationId) && showSearch && query.trim().length >= 2,
    staleTime: 10_000,
  });
  const assignMutation = useMutation({
    mutationFn: (resource: LinearIssueSummary) =>
      linearApi.assignAgentConversationLinearIssue({
        conversationId: conversationId!,
        projectId,
        issueId: resource.id,
        issueKey: resource.key ?? null,
        title: resource.title,
        issueUrl: resource.url ?? null,
      }),
    onSuccess: (assigned) => {
      queryClient.setQueryData(agentLinearIssueKeys.issue(conversationId), assigned);
      setIsReassigning(false);
      setQuery("");
      toast.success("Linear issue assigned");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to assign Linear issue");
    },
  });
  const refreshMutation = useMutation({
    mutationFn: (_options?: RefreshLinearIssueOptions) =>
      linearApi.refreshAgentConversationLinearIssue({
        conversationId: conversationId!,
      }),
    onSuccess: (refreshed, options) => {
      queryClient.setQueryData(agentLinearIssueKeys.issue(conversationId), refreshed);
      if (options?.silent) {
        return;
      }
      toast.success("Linear issue refreshed");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to refresh Linear issue");
    },
  });
  const refreshIssue = refreshMutation.mutate;

  useEffect(() => {
    if (
      !conversationId ||
      issue?.conversationId !== conversationId ||
      issue.refreshStatus !== "not_loaded" ||
      refreshMutation.isPending
    ) {
      return;
    }
    refreshIssue({ silent: true });
  }, [
    conversationId,
    issue?.conversationId,
    issue?.issueId,
    issue?.refreshStatus,
    refreshMutation.isPending,
    refreshIssue,
  ]);
  const clearMutation = useMutation({
    mutationFn: () =>
      linearApi.clearAgentConversationLinearIssue({
        conversationId: conversationId!,
      }),
    onSuccess: () => {
      queryClient.setQueryData(agentLinearIssueKeys.issue(conversationId), null);
      setIsReassigning(false);
      toast.success("Linear issue unlinked");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to unlink Linear issue");
    },
  });
  const isMutating =
    assignMutation.isPending || refreshMutation.isPending || clearMutation.isPending;
  const handleAssign = useCallback(
    (resource: LinearIssueSummary) => {
      if (!conversationId || isMutating) return;
      assignMutation.mutate(resource);
    },
    [assignMutation, conversationId, isMutating],
  );

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
        style={{ borderColor: "var(--border-subtle)", borderStyle: "solid", borderWidth: 0, borderBottomWidth: 1 }}
      >
        <div className="flex min-w-0 items-center gap-2">
          <Ticket className="h-4 w-4 shrink-0" style={{ color: "var(--accent-primary)" }} />
          <div className="min-w-0">
            <h2 className="truncate text-sm font-semibold">Linear</h2>
            <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
              {issue ? issue.issueKey ?? issue.issueId : "No issue assigned"}
            </p>
          </div>
        </div>
        {issue ? (
          <div className="flex items-center gap-1">
            {issue.issueUrl ? (
              <IconButton
                label="Open Linear issue"
                onClick={() => window.open(issue.issueUrl ?? undefined, "_blank", "noopener,noreferrer")}
              >
                <ExternalLink className="h-4 w-4" />
              </IconButton>
            ) : null}
            <IconButton
              label="Refresh Linear issue"
              onClick={() => refreshIssue({ silent: false })}
              disabled={!conversationId || isMutating}
            >
              <RefreshCw className={cn("h-4 w-4", refreshMutation.isPending && "animate-spin")} />
            </IconButton>
            <IconButton
              label="Unlink Linear issue"
              onClick={() => clearMutation.mutate()}
              disabled={!conversationId || isMutating}
            >
              <Unlink className="h-4 w-4" />
            </IconButton>
          </div>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {!conversationId ? (
          <PanelStatus label="No conversation selected" busy={false} />
        ) : issueQuery.isLoading ? (
          <PanelStatus label="Loading Linear issue" />
        ) : issue ? (
          <IssueDetails issue={issue} onReassign={() => setIsReassigning(true)} />
        ) : null}

        {conversationId && showSearch ? (
          <div className={cn("space-y-3", issue && "mt-4 border-t pt-4")} style={{ borderColor: "var(--border-subtle)" }}>
            <div className="relative">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2" style={{ color: "var(--text-muted)" }} />
              <Input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="Search Linear issues"
                className="pl-9"
              />
            </div>
            {searchQuery.isFetching ? <PanelStatus label="Searching Linear" /> : null}
            <div className="space-y-2">
              {(searchQuery.data ?? []).map((resource) => (
                <button
                  key={linearSearchKey(resource)}
                  type="button"
                  className="w-full rounded-md border px-3 py-2 text-left transition-colors"
                  style={{
                    backgroundColor: "var(--bg-surface)",
                    borderColor: "var(--border-subtle)",
                    borderStyle: "solid",
                    borderWidth: 1,
                  }}
                  onClick={() => handleAssign(resource)}
                  disabled={isMutating}
                >
                  <div className="flex items-center justify-between gap-3">
                    <span className="truncate text-sm font-medium">
                      {resource.key ?? resource.id}
                    </span>
                    {resource.stateName ? (
                      <span className="shrink-0 text-xs" style={{ color: "var(--text-muted)" }}>
                        {resource.stateName}
                      </span>
                    ) : null}
                  </div>
                  <p className="mt-1 line-clamp-2 text-xs" style={{ color: "var(--text-secondary)" }}>
                    {resource.title}
                  </p>
                </button>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function IssueDetails({
  issue,
  onReassign,
}: {
  issue: AgentConversationLinearIssue;
  onReassign: () => void;
}) {
  return (
    <ArtifactSelectableRegion
      className="space-y-4"
      source={{
        sourceKind: "linear",
        sourceId: issue.issueId,
        sourceLabel: `Linear ${issue.issueKey ?? issue.issueId}`,
        ...(issue.title ? { title: issue.title } : {}),
        ...(issue.issueUrl ? { url: issue.issueUrl } : {}),
      }}
    >
      <div>
        <div className="flex items-center justify-between gap-3">
          <h3 className="min-w-0 truncate text-base font-semibold">
            {issue.issueKey ?? issue.issueId}
          </h3>
          {issue.status ? (
            <span className="shrink-0 rounded px-2 py-1 text-xs" style={{ backgroundColor: "var(--overlay-faint)", color: "var(--text-secondary)" }}>
              {issue.status}
            </span>
          ) : null}
        </div>
        {issue.title ? (
          <p className="mt-1 text-sm" style={{ color: "var(--text-secondary)" }}>
            {issue.title}
          </p>
        ) : null}
      </div>
      <div className="grid gap-2 text-xs" style={{ color: "var(--text-muted)" }}>
        {issue.assignee ? <span>Assignee: {issue.assignee}</span> : null}
        {issue.reporter ? <span>Creator: {issue.reporter}</span> : null}
        {issue.updatedAtRemote ? <span>Updated: {formatDate(issue.updatedAtRemote)}</span> : null}
      </div>
      {issue.descriptionMarkdown || issue.descriptionText ? (
        <div
          className="rounded-md border p-3 text-sm"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: 1,
          }}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
            {issue.descriptionMarkdown ?? issue.descriptionText ?? ""}
          </ReactMarkdown>
        </div>
      ) : null}
      {issue.refreshStatus === "error" && issue.refreshError ? (
        <p className="text-xs" style={{ color: "var(--danger)" }}>
          {issue.refreshError}
        </p>
      ) : null}
      <Button type="button" variant="outline" size="sm" onClick={onReassign}>
        Reassign
      </Button>
    </ArtifactSelectableRegion>
  );
}

function PanelStatus({ label, busy = true }: { label: string; busy?: boolean }) {
  return (
    <div className="flex items-center gap-2 text-sm" style={{ color: "var(--text-muted)" }}>
      {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
      <span>{label}</span>
    </div>
  );
}

function IconButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function linearSearchKey(resource: LinearIssueSummary): string {
  return resource.key ? `linear:${resource.id}:${resource.key}` : `linear:${resource.id}`;
}

function formatDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}
