import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ExternalLink,
  Loader2,
  MessageSquare,
  Paperclip,
  RefreshCw,
  Search,
  Ticket,
  Unlink,
} from "lucide-react";
import {
  useCallback,
  useMemo,
  useState,
  type ComponentType,
  type CSSProperties,
  type ReactNode,
} from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";

import {
  atlassianApi,
  type AgentConversationJiraIssue,
  type AtlassianResourceSummary,
} from "@/api/atlassian";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useAgentComposerIntegrationResources } from "@/hooks/useAgentComposerResources";

const jiraIssueKey = (conversationId: string | null) =>
  ["agents", "jira-issue", conversationId] as const;

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
  const issueQuery = useQuery({
    queryKey: jiraIssueKey(conversationId),
    queryFn: () =>
      atlassianApi.getAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId),
    staleTime: 5_000,
  });
  const issue = issueQuery.data ?? null;
  const showSearch = !issue || isReassigning;
  const searchQuery = useAgentComposerIntegrationResources({
    kind: "jira",
    query,
    enabled: showSearch && query.trim().length >= 2,
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
      queryClient.setQueryData(jiraIssueKey(conversationId), assigned);
      setIsReassigning(false);
      setQuery("");
      toast.success("Jira issue assigned");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to assign Jira issue");
    },
  });
  const refreshMutation = useMutation({
    mutationFn: () =>
      atlassianApi.refreshAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    onSuccess: (refreshed) => {
      queryClient.setQueryData(jiraIssueKey(conversationId), refreshed);
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
  const clearMutation = useMutation({
    mutationFn: () =>
      atlassianApi.clearAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    onSuccess: () => {
      queryClient.setQueryData(jiraIssueKey(conversationId), null);
      setIsReassigning(false);
      toast.success("Jira issue unlinked");
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "Failed to unlink Jira issue");
    },
  });

  const isMutating =
    assignMutation.isPending || refreshMutation.isPending || clearMutation.isPending;

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
          background: "var(--bg-surface)",
        }}
      >
        <div
          className="flex h-9 w-9 items-center justify-center rounded-md border"
          style={{
            borderColor: "var(--border-subtle)",
            background: "var(--bg-base)",
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
                  background: "var(--accent-muted)",
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
              onClick={() => refreshMutation.mutate()}
            >
              <RefreshCw className={cn("h-4 w-4", refreshMutation.isPending && "animate-spin")} />
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

        {issue && <JiraIssueDetails issue={issue} />}
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

function JiraIssueDetails({ issue }: { issue: AgentConversationJiraIssue }) {
  const sections = useMemo(
    () => [
      {
        title: "Description",
        markdown: issue.descriptionMarkdown,
        text: issue.descriptionText,
      },
      {
        title: "Acceptance Criteria",
        markdown: issue.acceptanceCriteriaMarkdown,
        text: issue.acceptanceCriteriaText,
      },
    ],
    [
      issue.acceptanceCriteriaMarkdown,
      issue.acceptanceCriteriaText,
      issue.descriptionMarkdown,
      issue.descriptionText,
    ],
  );

  return (
    <div className="space-y-4">
      {issue.refreshStatus === "error" && issue.refreshError && (
        <PanelNotice title="Refresh failed" detail={issue.refreshError} tone="warning" />
      )}
      <div className="grid gap-2 text-xs sm:grid-cols-2">
        <Meta label="Assignee" value={issue.assignee} />
        <Meta label="Reporter" value={issue.reporter} />
        <Meta label="Updated" value={formatDate(issue.updatedAtRemote)} />
        <Meta label="Refreshed" value={formatDate(issue.lastRefreshedAt)} />
      </div>
      {sections.map((section) => (
        <RichSection
          key={section.title}
          title={section.title}
          markdown={section.markdown}
          text={section.text}
        />
      ))}
      <section className="space-y-2">
        <SectionHeader icon={MessageSquare} title={`Comments (${issue.comments.length})`} />
        {issue.comments.length === 0 ? (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No comments cached.
          </p>
        ) : (
          <div className="space-y-2">
            {issue.comments.map((comment, index) => (
              <div
                key={comment.id ?? index}
                className="rounded-md border p-3"
                style={{
                  borderColor: "var(--border-subtle)",
                  background: "var(--bg-surface)",
                }}
              >
                <div className="mb-2 flex items-center justify-between gap-3 text-xs">
                  <span className="font-medium" style={{ color: "var(--text-primary)" }}>
                    {comment.author ?? "Jira user"}
                  </span>
                  <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
                    {formatDate(comment.updatedAt ?? comment.createdAt)}
                  </span>
                </div>
                <MarkdownBody markdown={comment.bodyMarkdown} fallback={comment.bodyText} />
              </div>
            ))}
          </div>
        )}
      </section>
      <section className="space-y-2">
        <SectionHeader icon={Paperclip} title={`Attachments (${issue.attachments.length})`} />
        {issue.attachments.length === 0 ? (
          <p className="text-xs" style={{ color: "var(--text-muted)" }}>
            No attachments cached.
          </p>
        ) : (
          <div
            className="overflow-hidden rounded-md border"
            style={{ borderColor: "var(--border-subtle)" }}
          >
            {issue.attachments.map((attachment) => (
              <a
                key={attachment.id ?? attachment.filename}
                href={attachment.contentUrl ?? undefined}
                target="_blank"
                rel="noreferrer"
                className="flex items-center gap-3 border-b px-3 py-2 text-sm last:border-b-0"
                style={{
                  borderColor: "var(--border-subtle)",
                  color: "var(--text-primary)",
                }}
              >
                <Paperclip className="h-4 w-4 shrink-0" style={{ color: "var(--text-muted)" }} />
                <span className="min-w-0 flex-1 truncate">{attachment.filename}</span>
                {typeof attachment.size === "number" && (
                  <span className="shrink-0 text-xs" style={{ color: "var(--text-muted)" }}>
                    {formatBytes(attachment.size)}
                  </span>
                )}
              </a>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

function RichSection({
  title,
  markdown,
  text,
}: {
  title: string;
  markdown?: string | null | undefined;
  text?: string | null | undefined;
}) {
  return (
    <section className="space-y-2">
      <SectionHeader icon={Ticket} title={title} />
      {markdown || text ? (
        <MarkdownBody markdown={markdown} fallback={text} />
      ) : (
        <p className="text-xs" style={{ color: "var(--text-muted)" }}>
          Not cached.
        </p>
      )}
    </section>
  );
}

function MarkdownBody({
  markdown,
  fallback,
}: {
  markdown?: string | null | undefined;
  fallback?: string | null | undefined;
}) {
  const content = markdown || fallback || "";
  return (
    <div
      className="prose prose-sm max-w-none text-[0.8125rem] leading-relaxed prose-code:before:content-none prose-code:after:content-none"
      style={{ color: "var(--text-primary)" }}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {content}
      </ReactMarkdown>
    </div>
  );
}

function SectionHeader({
  icon: Icon,
  title,
}: {
  icon: ComponentType<{ className?: string; style?: CSSProperties }>;
  title: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <Icon className="h-4 w-4" style={{ color: "var(--text-muted)" }} />
      <h3 className="text-xs font-semibold uppercase" style={{ color: "var(--text-muted)" }}>
        {title}
      </h3>
    </div>
  );
}

function Meta({
  label,
  value,
}: {
  label: string;
  value?: string | null | undefined;
}) {
  return (
    <div
      className="rounded-md border px-3 py-2"
      style={{
        borderColor: "var(--border-subtle)",
        background: "var(--bg-surface)",
      }}
    >
      <div className="text-[0.6875rem] uppercase" style={{ color: "var(--text-muted)" }}>
        {label}
      </div>
      <div className="mt-1 truncate font-medium" style={{ color: "var(--text-primary)" }}>
        {value || "Unknown"}
      </div>
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

function PanelNotice({
  title,
  detail,
  busy = false,
  tone = "default",
}: {
  title: string;
  detail: string;
  busy?: boolean;
  tone?: "default" | "warning";
}) {
  return (
    <div
      className="rounded-md border px-3 py-3"
      style={{
        borderColor:
          tone === "warning" ? "var(--status-warning)" : "var(--border-subtle)",
        background: "var(--bg-surface)",
      }}
    >
      <div className="flex items-center gap-2 text-sm font-medium" style={{ color: "var(--text-primary)" }}>
        {busy && <Loader2 className="h-4 w-4 animate-spin" />}
        <span>{title}</span>
      </div>
      <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
        {detail}
      </p>
    </div>
  );
}

function formatDate(value?: string | null): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}
