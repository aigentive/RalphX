import {
  BookOpen,
  ExternalLink,
  FileText,
  FolderOpen,
  ScrollText,
  Ticket,
} from "lucide-react";

import type { MessageComposerReferences } from "./MessageReferences.parse";

export function MessageReferences({
  projectReferences,
  integrationReferences,
  artifactReferences,
}: MessageComposerReferences) {
  if (
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0
  ) {
    return null;
  }

  return (
    <div
      data-testid="message-reference-list"
      className="mb-2 flex max-w-[min(85%,620px)] flex-wrap justify-end gap-2 self-end"
    >
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
        const isLinear = reference.kind === "linear";
        const label =
          isJira || isLinear
            ? (reference.key ?? reference.id)
            : (reference.title ?? reference.id);
        const description = isJira || isLinear ? reference.title : reference.id;
        const typeLabel = isLinear ? "Linear" : isJira ? "Jira" : "Confluence";
        return (
          <ReferenceChip
            key={`integration:${reference.provider}:${reference.kind}:${reference.id}`}
            testId={`message-reference-integration:${reference.kind}:${reference.id}`}
            icon={isJira || isLinear ? Ticket : BookOpen}
            typeLabel={typeLabel}
            label={label}
            {...(description && description !== label ? { description } : {})}
            {...(reference.url ? { url: reference.url } : {})}
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
    </div>
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
}: {
  testId: string;
  icon: typeof FileText;
  typeLabel: string;
  label: string;
  description?: string;
  url?: string;
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
