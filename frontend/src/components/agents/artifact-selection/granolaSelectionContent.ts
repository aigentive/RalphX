export interface GranolaSelectionContentInput {
  noteId: string;
  title?: string | null | undefined;
  summaryMarkdown?: string | null | undefined;
  transcript?: readonly unknown[] | null | undefined;
}

export function buildGranolaSelectionContent({
  noteId,
  title,
  summaryMarkdown,
  transcript,
}: GranolaSelectionContentInput): string {
  const heading = getGranolaSelectionSourceTitle(title) || normalizeInline(noteId);
  const lines = [`# ${heading}`];
  const summary = normalizeBlock(summaryMarkdown);
  if (summary) {
    lines.push("", "## Summary", "", summary);
  }

  const transcriptLines = (transcript ?? [])
    .map(buildTranscriptEntry)
    .filter((entry): entry is string => Boolean(entry));
  if (transcriptLines.length > 0) {
    lines.push("", "## Transcript", "", ...transcriptLines);
  }
  return lines.join("\n");
}

export function getGranolaSelectionSourceTitle(
  title: string | null | undefined,
): string | null {
  return normalizeInline(title) || null;
}

function buildTranscriptEntry(value: unknown): string | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const text = normalizeBlock(
    typeof record.text === "string" ? record.text : undefined,
  );
  if (!text) return null;
  const speaker = normalizeInline(
    typeof record.speaker === "string" ? record.speaker : undefined,
  );
  return speaker ? `${speaker}: ${text}` : text;
}

function normalizeBlock(value: string | null | undefined): string {
  return value?.replace(/\r\n?/g, "\n").trim() ?? "";
}

function normalizeInline(value: string | null | undefined): string {
  return value?.replace(/\s+/g, " ").trim() ?? "";
}
