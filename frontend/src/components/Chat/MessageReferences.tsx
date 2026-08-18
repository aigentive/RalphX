import {
  BookOpen,
  ExternalLink,
  FileText,
  FolderOpen,
  Kanban,
  Link2,
  ScrollText,
  Ticket,
} from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { composerExcerptReferenceKey } from "@/components/agents/artifact-selection/artifactSelection.types";
import { getComposerSelectionSourceLabel } from "@/lib/composer-selection-snapshot";
import { useTicketingStore } from "@/stores/ticketingStore";
import { useUiStore } from "@/stores/uiStore";
import type { MessageComposerReferences } from "./MessageReferences.parse";

export function MessageReferences({
  folderReferences = [],
  projectReferences,
  integrationReferences,
  artifactReferences,
  selectionSnapshot,
  excerptReferences = [],
}: MessageComposerReferences) {
  const setTicketProvider = useTicketingStore((s) => s.setProvider);
  const setSelectedTicketRef = useTicketingStore((s) => s.setSelectedTicketRef);
  const setCurrentView = useUiStore((s) => s.setCurrentView);

  if (
    folderReferences.length === 0 &&
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0 &&
    !selectionSnapshot &&
    excerptReferences.length === 0
  ) {
    return null;
  }

  return (
    <div
      data-testid="message-reference-list"
      className="mb-2 flex max-w-[min(85%,620px)] flex-wrap justify-end gap-2 self-end"
    >
      {folderReferences.map((reference) => (
        <ReferenceChip
          key={`folder:${reference.id ?? reference.folderPath}`}
          testId={`message-reference-folder:${reference.id ?? reference.folderPath}`}
          icon={FolderOpen}
          typeLabel="Folder"
          label={reference.displayName}
          description={reference.folderPath}
        />
      ))}
      {projectReferences.map((reference) => {
        const isFolder = reference.kind === "directory";
        return (
          <ReferenceChip
            key={`project:${reference.path}`}
            testId={`message-reference-project:${reference.path}`}
            icon={isFolder ? FolderOpen : FileText}
            typeLabel={isFolder ? "Folder" : "File"}
            label={reference.path}
          />
        );
      })}
      {integrationReferences.map((reference) => {
        const isJira = reference.kind === "jira";
        const isJiraBoard = reference.kind === "jira_board";
        const isLinear = reference.kind === "linear";
        const isClickUp = reference.kind === "clickup";
        const isConfluenceLink = reference.kind === "confluence_link";
        const isGranola = reference.provider === "granola" && reference.kind === "note";
        const ticketProvider = isClickUp
          ? "clickup"
          : isLinear
            ? "linear"
            : isJira
              ? "jira"
              : null;
        const label =
          isJira || isLinear || isClickUp
            ? (reference.key ?? reference.id)
            : (reference.title ?? reference.id);
        const description =
          isJira || isLinear || isClickUp
            ? reference.title
            : isGranola
              ? reference.summaryExcerpt
              : reference.id;
        const typeLabel = isClickUp
          ? "ClickUp"
          : isLinear
            ? "Linear"
            : isJira
              ? "Jira"
              : isJiraBoard
                ? "Jira Board"
                : isGranola
                  ? "Granola"
                  : isConfluenceLink
                    ? "Confluence Link"
                    : "Confluence";
        const icon = isJira || isLinear || isClickUp
          ? Ticket
          : isJiraBoard
            ? Kanban
            : isGranola
              ? ScrollText
              : isConfluenceLink
                ? Link2
                : BookOpen;
        return (
          <ReferenceChip
            key={`integration:${reference.provider}:${reference.kind}:${reference.id}`}
            testId={`message-reference-integration:${reference.kind}:${reference.id}`}
            icon={icon}
            typeLabel={typeLabel}
            label={label}
            {...(description && description !== label ? { description } : {})}
            {...(reference.url ? { url: reference.url } : {})}
            {...(ticketProvider
              ? {
                  onOpenTicket: () => {
                    setTicketProvider(ticketProvider);
                    setSelectedTicketRef({
                      provider: ticketProvider,
                      id: reference.id,
                      ...(reference.key ? { key: reference.key } : {}),
                    });
                    setCurrentView("ticketing");
                  },
                }
              : {})}
          />
        );
      })}
      {artifactReferences.map((reference) => {
        const label = reference.title ?? shortReferenceId(reference.artifactId);
        const description = [
          reference.status
            ? formatArtifactReferenceStatus(reference.status)
            : null,
          reference.version ? `v${reference.version}` : null,
        ]
          .filter(Boolean)
          .join(" · ");
        return (
          <ReferenceChip
            key={`artifact:${reference.kind}:${reference.artifactId}`}
            testId={`message-reference-artifact:${reference.kind}:${reference.artifactId}`}
            icon={ScrollText}
            typeLabel={reference.kind === "plan" ? "Plan" : "Artifact"}
            label={label}
            {...(description ? { description } : {})}
          />
        );
      })}
      {selectionSnapshot ? (
        <SelectionSnapshotReference snapshot={selectionSnapshot} />
      ) : null}
      {excerptReferences.map((reference) => {
        const label = reference.title ?? reference.sourceId;
        const description = [
          reference.version !== undefined ? `v${reference.version}` : null,
          reference.filePath,
          reference.locator,
          reference.excerpt,
        ]
          .filter(Boolean)
          .join(" · ");
        return (
          <ReferenceChip
            key={`excerpt:${composerExcerptReferenceKey(reference)}`}
            testId={`message-reference-excerpt:${reference.sourceKind}:${reference.sourceId}`}
            icon={ScrollText}
            typeLabel={`${reference.sourceLabel} excerpt`}
            label={label}
            {...(description ? { description } : {})}
          />
        );
      })}
    </div>
  );
}

function SelectionSnapshotReference({
  snapshot,
}: {
  snapshot: NonNullable<MessageComposerReferences["selectionSnapshot"]>;
}) {
  const sourceLabel = getComposerSelectionSourceLabel(snapshot);
  const lineLabel =
    snapshot.startLine === snapshot.endLine
      ? `L${snapshot.startLine}`
      : `L${snapshot.startLine}–${snapshot.endLine}`;

  return (
    <Dialog>
      <div
        className="inline-flex max-w-full min-w-0 items-center gap-2 rounded-md border px-2 py-1 text-xs"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--bg-hover)",
          borderStyle: "solid",
          borderWidth: 1,
          color: "var(--text-primary)",
        }}
        data-testid="message-selection-snapshot"
      >
        <ScrollText className="h-3 w-3 shrink-0" />
        <span
          className="truncate whitespace-nowrap"
          title={`Selection · ${sourceLabel} · ${lineLabel}`}
        >{`Selection · ${sourceLabel} · ${lineLabel}`}</span>
        <DialogTrigger asChild>
          <button
            type="button"
            className="rounded px-1.5 py-0.5 font-medium hover:bg-[var(--bg-hover)]"
            aria-label="View selection snapshot"
          >
            View
          </button>
        </DialogTrigger>
      </div>
      <DialogContent className="max-h-[min(80vh,720px)] max-w-3xl overflow-hidden">
        <DialogHeader className="block pr-12">
          <DialogTitle>{sourceLabel}</DialogTitle>
          <DialogDescription>
            Frozen selection · {lineLabel}
          </DialogDescription>
        </DialogHeader>
        <div
          className="max-h-[60vh] overflow-auto px-4 py-4 font-mono text-xs"
          style={{ backgroundColor: "var(--bg-base)" }}
        >
          {snapshot.content.split("\n").map((line, index) => (
            <div key={snapshot.startLine + index} className="flex min-w-max leading-5">
              <span
                className="mr-4 w-10 shrink-0 select-none text-right tabular-nums"
                style={{ color: "var(--text-muted)" }}
              >
                {snapshot.startLine + index}
              </span>
              <span className="whitespace-pre" style={{ color: "var(--text-primary)" }}>
                {line || " "}
              </span>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function formatArtifactReferenceStatus(status: string): string {
  if (status === "approved") {
    return "Approved";
  }
  if (status === "accepted") {
    return "Accepted";
  }
  return "Draft";
}

function shortReferenceId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 8)}...` : id;
}

function ReferenceChip({
  testId,
  icon: Icon,
  typeLabel,
  label,
  description,
  url,
  onOpenTicket,
}: {
  testId: string;
  icon: typeof FileText;
  typeLabel: string;
  label: string;
  description?: string;
  url?: string;
  onOpenTicket?: () => void;
}) {
  const content = (
    <>
      <Icon className="h-3 w-3 shrink-0 text-[var(--text-secondary)]" />
      <span className="shrink-0 rounded border px-1 py-0.5 text-[0.5625rem] font-medium uppercase text-[var(--text-muted)]">
        {typeLabel}
      </span>
      <span
        className="min-w-0 max-w-full break-words"
        style={{ overflowWrap: "anywhere" }}
        title={label}
      >
        {label}
      </span>
      {description ? (
        <span
          className="hidden min-w-0 max-w-full break-words text-[var(--text-muted)] sm:inline"
          style={{ overflowWrap: "anywhere" }}
          title={description}
        >
          {description}
        </span>
      ) : null}
      {url ? (
        <ExternalLink className="h-3 w-3 shrink-0 text-[var(--text-muted)]" />
      ) : null}
    </>
  );
  const className =
    "inline-flex max-w-full min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1 rounded-md border px-2 py-1 text-left text-xs no-underline hover:no-underline focus-visible:no-underline";
  const style = {
    backgroundColor: "var(--bg-elevated)",
    borderColor: "var(--bg-hover)",
    borderStyle: "solid",
    borderWidth: "1px",
    color: "var(--text-primary)",
    textDecoration: "none",
  };

  if (onOpenTicket) {
    return (
      <button
        type="button"
        data-testid={testId}
        className={className}
        style={style}
        title={label}
        onClick={onOpenTicket}
      >
        {content}
      </button>
    );
  }

  if (url) {
    return (
      <a
        data-testid={testId}
        href={url}
        target="_blank"
        rel="noreferrer"
        className={className}
        style={style}
        title={label}
      >
        {content}
      </a>
    );
  }

  return (
    <span
      data-testid={testId}
      className={className}
      style={style}
      title={label}
    >
      {content}
    </span>
  );
}
