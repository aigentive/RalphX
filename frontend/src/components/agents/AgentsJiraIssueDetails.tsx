import {
  ListChecks,
  Loader2,
  MessageSquare,
  Paperclip,
  Ticket,
  UserCheck,
} from "lucide-react";
import { useMemo, type ComponentType, type CSSProperties } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type { AgentConversationJiraIssue } from "@/api/atlassian";
import { Button } from "@/components/ui/button";

import { ArtifactSelectableRegion } from "./artifact-selection/ArtifactSelectableRegion";
import { jiraMarkdownComponents } from "./AgentsJiraIssuePanel.markdown";

export function JiraIssueDetails({
  issue,
  isAssigningToMe,
  onAssignToMe,
}: {
  issue: AgentConversationJiraIssue;
  isAssigningToMe: boolean;
  onAssignToMe: () => void;
}) {
  const sections = useMemo(
    () =>
      [
        {
          title: "Description",
          markdown: issue.descriptionMarkdown,
          text: issue.descriptionText,
        },
      ].filter((section) => hasRichText(section.markdown, section.text)),
    [issue.descriptionMarkdown, issue.descriptionText],
  );

  return (
    <ArtifactSelectableRegion
      className="space-y-4"
      source={{
        sourceKind: "jira",
        sourceId: issue.issueId ?? issue.issueKey,
        sourceLabel: `Jira ${issue.issueKey}`,
        ...(issue.title ? { title: issue.title } : {}),
        ...(issue.issueUrl ? { url: issue.issueUrl } : {}),
      }}
    >
      {issue.refreshStatus === "error" && issue.refreshError && (
        <PanelNotice title="Refresh failed" detail={issue.refreshError} tone="warning" />
      )}
      <div
        aria-label="Jira issue metadata"
        className="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs"
      >
        <AssigneeMeta
          value={issue.assignee}
          isAssigningToMe={isAssigningToMe}
          onAssignToMe={onAssignToMe}
        />
        <InlineMeta label="Reporter" value={issue.reporter} />
        <InlineMeta label="Updated" value={formatDate(issue.updatedAtRemote)} />
        <InlineMeta label="Refreshed" value={formatDate(issue.lastRefreshedAt)} />
      </div>
      {sections.map((section) => (
        <RichSection
          key={section.title}
          title={section.title}
          markdown={section.markdown}
          text={section.text}
        />
      ))}
      {hasRichText(issue.acceptanceCriteriaMarkdown, issue.acceptanceCriteriaText) ? (
        <RichSection
          icon={ListChecks}
          title="Acceptance Criteria"
          markdown={issue.acceptanceCriteriaMarkdown}
          text={issue.acceptanceCriteriaText}
        />
      ) : issue.refreshStatus === "loaded" ? (
        <section className="space-y-2">
          <SectionHeader icon={ListChecks} title="Acceptance Criteria" />
          <p className="text-[0.8125rem]" style={{ color: "var(--text-muted)" }}>
            No acceptance criteria on this issue.
          </p>
        </section>
      ) : null}
      {issue.comments.length > 0 && (
        <section className="space-y-2">
          <SectionHeader icon={MessageSquare} title={`Comments (${issue.comments.length})`} />
          <div className="space-y-2">
            {issue.comments.map((comment, index) => (
              <div
                key={comment.id ?? index}
                className="rounded-md border p-3"
                style={{
                  borderColor: "var(--border-subtle)",
                  backgroundColor: "var(--bg-surface)",
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
        </section>
      )}
      {issue.attachments.length > 0 && (
        <section className="space-y-2">
          <SectionHeader icon={Paperclip} title={`Attachments (${issue.attachments.length})`} />
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
        </section>
      )}
    </ArtifactSelectableRegion>
  );
}

function RichSection({
  icon = Ticket,
  title,
  markdown,
  text,
}: {
  icon?: ComponentType<{ className?: string; style?: CSSProperties }>;
  title: string;
  markdown?: string | null | undefined;
  text?: string | null | undefined;
}) {
  if (!hasRichText(markdown, text)) {
    return null;
  }
  return (
    <section className="space-y-2">
      <SectionHeader icon={icon} title={title} />
      <MarkdownBody markdown={markdown} fallback={text} />
    </section>
  );
}

function hasRichText(
  markdown?: string | null | undefined,
  text?: string | null | undefined,
): boolean {
  return Boolean((markdown ?? text ?? "").trim());
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
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={jiraMarkdownComponents}>
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

function InlineMeta({
  label,
  value,
}: {
  label: string;
  value?: string | null | undefined;
}) {
  return (
    <div className="inline-flex min-w-0 items-center gap-1.5">
      <span className="shrink-0 text-[0.6875rem] uppercase" style={{ color: "var(--text-muted)" }}>
        {label}
      </span>
      <span className="truncate font-medium" style={{ color: "var(--text-primary)" }}>
        {value || "Unknown"}
      </span>
    </div>
  );
}

function AssigneeMeta({
  value,
  isAssigningToMe,
  onAssignToMe,
}: {
  value?: string | null | undefined;
  isAssigningToMe: boolean;
  onAssignToMe: () => void;
}) {
  const isUnassigned = isUnassignedAssignee(value);
  return (
    <div className="inline-flex min-w-0 items-center gap-1.5">
      <span className="shrink-0 text-[0.6875rem] uppercase" style={{ color: "var(--text-muted)" }}>
        Assignee
      </span>
      <span className="truncate font-medium" style={{ color: "var(--text-primary)" }}>
        {isUnassigned ? "Unassigned" : value?.trim()}
      </span>
      {isUnassigned && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-6 gap-1 px-1.5 text-xs"
          disabled={isAssigningToMe}
          onClick={onAssignToMe}
        >
          {isAssigningToMe ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <UserCheck className="h-3.5 w-3.5" />
          )}
          Assign to me
        </Button>
      )}
    </div>
  );
}

function isUnassignedAssignee(value?: string | null | undefined): boolean {
  const normalized = value?.trim().toLowerCase();
  return !normalized || normalized === "unknown" || normalized === "unassigned";
}

export function PanelNotice({
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
        backgroundColor: "var(--bg-surface)",
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
