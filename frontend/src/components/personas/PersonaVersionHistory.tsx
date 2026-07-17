import { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import {
  usePersonaArtifactHistory,
  usePersonaArtifactVersion,
} from "@/hooks/usePersonaArtifact";
import type { PersonaArtifactVersionSummary } from "@/types/artifact";

function attributionLabel(version: PersonaArtifactVersionSummary): string {
  const author = version.metadata?.created_by ?? version.created_by;
  const attribution =
    author === "user"
      ? "you (manual edit)"
      : author === "agent"
        ? "agent"
        : author;
  const personaVersion = version.metadata?.persona_version ?? version.version;
  const timestamp = new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(version.created_at));
  return `v${personaVersion} ${attribution} · ${timestamp}`;
}

export function PersonaVersionHistory({
  artifactId,
  currentContent,
  selectedVersion,
  onSelectedVersionChange,
  selectId = "persona-artifact-version",
}: {
  artifactId: string;
  currentContent: string | null | undefined;
  selectedVersion: number | null;
  onSelectedVersionChange: (version: number | null) => void;
  selectId?: string;
}) {
  const historyQuery = usePersonaArtifactHistory(artifactId);
  const historicalQuery = usePersonaArtifactVersion(artifactId, selectedVersion);
  const orderedHistory = useMemo(
    () => [...(historyQuery.data ?? [])].sort((left, right) => right.version - left.version),
    [historyQuery.data],
  );
  const historicalContent =
    historicalQuery.data?.content.type === "inline"
      ? historicalQuery.data.content.text
      : null;
  const content = selectedVersion == null ? currentContent : historicalContent;
  const isHistorical = selectedVersion != null;

  return (
    <>
      {orderedHistory.length > 0 && (
        <div>
          <label
            htmlFor={selectId}
            className="mb-1.5 block text-xs font-medium text-[var(--text-secondary)]"
          >
            Version
          </label>
          <select
            id={selectId}
            aria-label="Persona version"
            value={selectedVersion?.toString() ?? "current"}
            onChange={(event) => {
              onSelectedVersionChange(
                event.target.value === "current" ? null : Number(event.target.value),
              );
            }}
            className="h-9 w-full rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-2 text-xs text-[var(--text-primary)] outline-none"
          >
            <option value="current">
              Current · {attributionLabel(orderedHistory[0]!)}
            </option>
            {orderedHistory.slice(1).map((version) => (
              <option key={version.id} value={version.version}>
                {attributionLabel(version)}
              </option>
            ))}
          </select>
        </div>
      )}

      {isHistorical && (
        <p className="mt-3 text-xs font-medium text-[var(--text-muted)]">
          Historical version · read-only
        </p>
      )}

      <article className="prose prose-sm mt-5 max-w-none text-[var(--text-primary)] prose-headings:text-[var(--text-primary)] prose-p:text-[var(--text-secondary)]">
        {isHistorical && historicalQuery.isPending ? (
          <div
            className="h-28 animate-pulse rounded bg-[var(--bg-elevated)]"
            aria-label="Loading historical persona"
          />
        ) : content ? (
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
        ) : (
          <p className="text-sm text-[var(--text-muted)]">
            This version has no inline content.
          </p>
        )}
      </article>
    </>
  );
}
