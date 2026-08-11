import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ExternalLink,
  Loader2,
  RefreshCw,
  Search,
  Ticket,
  Unlink,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useState,
  type ReactNode,
} from "react";
import { toast } from "sonner";

import {
  atlassianApi,
  type AtlassianResourceSummary,
} from "@/api/atlassian";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { agentJiraIssueKeys } from "./agentJiraIssueQueries";
import { JiraIssueDetails, PanelNotice } from "./AgentsJiraIssueDetails";

export { JiraIssueDetails } from "./AgentsJiraIssueDetails";

type RefreshJiraIssueOptions = {
  silent?: boolean;
};


interface AgentsJiraIssuePanelProps {
  conversationId: string | null;
  projectId: string | null;
}

export function AgentsJiraIssuePanel({
  conversationId,
  projectId,
}: AgentsJiraIssuePanelProps) {
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [isReassigning, setIsReassigning] = useState(false);
  const normalizedSearchQuery = query.trim();
  const issueQuery = useQuery({
    queryKey: agentJiraIssueKeys.issue(conversationId),
    queryFn: () =>
      atlassianApi.getAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId),
    staleTime: 5_000,
  });
  const issue = issueQuery.data ?? null;
  const showSearch = !issue || isReassigning;
  const searchQuery = useQuery({
    queryKey: ["agent-conversation-jira-issue", "search", normalizedSearchQuery],
    queryFn: () =>
      atlassianApi.searchResources({
        kind: "jira",
        query: normalizedSearchQuery,
        limit: 12,
      }),
    enabled: showSearch && normalizedSearchQuery.length >= 2,
    staleTime: 10_000,
    gcTime: 60_000,
    placeholderData: [] satisfies AtlassianResourceSummary[],
  });
  const assignMutation = useMutation({
    mutationFn: (resource: AtlassianResourceSummary) =>
      atlassianApi.assignAgentConversationJiraIssue({
        conversationId: conversationId!,
        projectId,
        issueKey: resource.key ?? resource.id,
        issueId: resource.id,
        title: resource.title,
        issueUrl: resource.url ?? null,
      }),
    onSuccess: (assigned) => {
      queryClient.setQueryData(agentJiraIssueKeys.issue(conversationId), assigned);
      setIsReassigning(false);
      setQuery("");
      toast.success("Jira issue assigned");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to assign Jira issue");
    },
  });
  const refreshMutation = useMutation({
    mutationFn: (_options?: RefreshJiraIssueOptions) =>
      atlassianApi.refreshAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    onSuccess: (refreshed, options) => {
      queryClient.setQueryData(agentJiraIssueKeys.issue(conversationId), refreshed);
      if (options?.silent) {
        return;
      }
      if (refreshed?.refreshStatus === "error") {
        toast.error(refreshed.refreshError ?? "Failed to refresh Jira issue");
      } else {
        toast.success("Jira issue refreshed");
      }
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to refresh Jira issue");
    },
  });
  const assignToMeMutation = useMutation({
    mutationFn: () =>
      atlassianApi.assignAgentConversationJiraIssueToMe({
        conversationId: conversationId!,
      }),
    onSuccess: (assigned) => {
      queryClient.setQueryData(agentJiraIssueKeys.issue(conversationId), assigned);
      toast.success("Jira issue assigned to you");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to assign Jira issue to you");
    },
  });
  const clearMutation = useMutation({
    mutationFn: () =>
      atlassianApi.clearAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    onSuccess: () => {
      queryClient.setQueryData(agentJiraIssueKeys.issue(conversationId), null);
      setIsReassigning(false);
      toast.success("Jira issue unlinked");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to unlink Jira issue");
    },
  });

  const refreshIssue = refreshMutation.mutate;
  const isRefreshingIssue = refreshMutation.isPending;
  const isMutating =
    assignMutation.isPending ||
    isRefreshingIssue ||
    assignToMeMutation.isPending ||
    clearMutation.isPending;

  useEffect(() => {
    if (
      !conversationId ||
      issue?.conversationId !== conversationId ||
      issue.refreshStatus !== "not_loaded" ||
      isRefreshingIssue
    ) {
      return;
    }
    refreshIssue({ silent: true });
  }, [
    conversationId,
    issue?.conversationId,
    issue?.issueKey,
    issue?.refreshStatus,
    isRefreshingIssue,
    refreshIssue,
  ]);

  const handleAssign = useCallback(
    (resource: AtlassianResourceSummary) => {
      if (!conversationId || isMutating) return;
      assignMutation.mutate(resource);
    },
    [assignMutation, conversationId, isMutating],
  );

  return (
    <div className="flex min-h-full flex-col">
      <div
        className="flex min-h-14 items-center gap-3 border-b px-4 py-3"
        style={{
          borderColor: "var(--border-subtle)",
          backgroundColor: "var(--bg-surface)",
        }}
      >
        <div
          className="flex h-9 w-9 items-center justify-center rounded-md border"
          style={{
            borderColor: "var(--border-subtle)",
            backgroundColor: "var(--bg-base)",
            color: "var(--accent-primary)",
          }}
        >
          <Ticket className="h-4 w-4" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
              {issue?.issueKey ?? "Jira"}
            </h2>
            {issue?.status && (
              <span
                className="shrink-0 rounded-full px-2 py-0.5 text-[0.6875rem] font-medium"
                style={{
                  backgroundColor: "var(--accent-muted)",
                  color: "var(--accent-primary)",
                }}
              >
                {issue.status}
              </span>
            )}
          </div>
          <p className="truncate text-xs" style={{ color: "var(--text-muted)" }}>
            {issue?.title ?? "No issue assigned"}
          </p>
        </div>
        {issue && (
          <div className="flex items-center gap-1">
            {issue.issueUrl && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button asChild type="button" variant="ghost" size="sm" className="h-8 w-8 p-0">
                    <a href={issue.issueUrl} target="_blank" rel="noreferrer" aria-label="Open in Jira">
                      <ExternalLink className="h-4 w-4" />
                    </a>
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs">
                  Open in Jira
                </TooltipContent>
              </Tooltip>
            )}
            <IconAction
              label="Refresh Jira issue"
              disabled={!conversationId || isMutating}
              onClick={() => refreshIssue({ silent: false })}
            >
              <RefreshCw className={cn("h-4 w-4", isRefreshingIssue && "animate-spin")} />
            </IconAction>
            <IconAction
              label="Reassign Jira issue"
              disabled={!conversationId || isMutating}
              onClick={() => setIsReassigning((value) => !value)}
            >
              <Search className="h-4 w-4" />
            </IconAction>
            <IconAction
              label="Unlink Jira issue"
              disabled={!conversationId || isMutating}
              onClick={() => clearMutation.mutate()}
            >
              <Unlink className="h-4 w-4" />
            </IconAction>
          </div>
        )}
      </div>

      <div className="flex-1 space-y-4 px-4 py-4">
        {!conversationId && (
          <PanelNotice title="No conversation selected" detail="Select an agent conversation." />
        )}

        {conversationId && issueQuery.isLoading && (
          <PanelNotice title="Loading Jira issue" detail="Fetching assignment state." busy />
        )}

        {conversationId && !issueQuery.isLoading && showSearch && (
          <JiraIssueSearch
            query={query}
            onQueryChange={setQuery}
            results={searchQuery.data ?? []}
            isSearching={searchQuery.isFetching}
            isAssigning={assignMutation.isPending}
            onAssign={handleAssign}
            onCancel={issue ? () => setIsReassigning(false) : undefined}
          />
        )}

        {issue && (
          <JiraIssueDetails
            issue={issue}
            isAssigningToMe={assignToMeMutation.isPending}
            onAssignToMe={() => {
              if (!conversationId || isMutating) return;
              assignToMeMutation.mutate();
            }}
          />
        )}
      </div>
    </div>
  );
}

function JiraIssueSearch({
  query,
  onQueryChange,
  results,
  isSearching,
  isAssigning,
  onAssign,
  onCancel,
}: {
  query: string;
  onQueryChange: (value: string) => void;
  results: AtlassianResourceSummary[];
  isSearching: boolean;
  isAssigning: boolean;
  onAssign: (resource: AtlassianResourceSummary) => void;
  onCancel?: (() => void) | undefined;
}) {
  return (
    <section className="space-y-3">
      <div className="flex items-center gap-2">
        <div className="relative min-w-0 flex-1">
          <Search
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2"
            style={{ color: "var(--text-muted)" }}
          />
          <Input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            placeholder="Search Jira issues"
            className="h-9 pl-9 text-sm"
            autoComplete="off"
          />
        </div>
        {onCancel && (
          <Button type="button" variant="ghost" size="sm" onClick={onCancel}>
            Cancel
          </Button>
        )}
      </div>
      <div
        className="overflow-hidden rounded-md border"
        style={{ borderColor: "var(--border-subtle)" }}
      >
        {query.trim().length < 2 ? (
          <SearchEmpty text="Enter at least 2 characters." />
        ) : isSearching ? (
          <SearchEmpty text="Searching..." busy />
        ) : results.length === 0 ? (
          <SearchEmpty text="No Jira issues found." />
        ) : (
          results.map((resource) => (
            <button
              key={`${resource.kind}:${resource.id}:${resource.key ?? ""}`}
              type="button"
              disabled={isAssigning}
              onClick={() => onAssign(resource)}
              className="flex w-full min-w-0 items-start gap-3 border-b px-3 py-2 text-left last:border-b-0 disabled:opacity-60"
              style={{ borderColor: "var(--border-subtle)" }}
            >
              <Ticket className="mt-0.5 h-4 w-4 shrink-0" style={{ color: "var(--accent-primary)" }} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium" style={{ color: "var(--text-primary)" }}>
                  {resource.key ?? resource.id}
                  {resource.title ? ` · ${resource.title}` : ""}
                </span>
                {resource.excerpt && (
                  <span className="mt-0.5 block truncate text-xs" style={{ color: "var(--text-muted)" }}>
                    {resource.excerpt}
                  </span>
                )}
              </span>
            </button>
          ))
        )}
      </div>
    </section>
  );
}

function SearchEmpty({ text, busy = false }: { text: string; busy?: boolean }) {
  return (
    <div className="flex items-center gap-2 px-3 py-3 text-xs" style={{ color: "var(--text-muted)" }}>
      {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
      <span>{text}</span>
    </div>
  );
}


function IconAction({
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
          size="sm"
          className="h-8 w-8 p-0"
          disabled={disabled}
          onClick={onClick}
          aria-label={label}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="text-xs">
        {label}
      </TooltipContent>
    </Tooltip>
  );
}
