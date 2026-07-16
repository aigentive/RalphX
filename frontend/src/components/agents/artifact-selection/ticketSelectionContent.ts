export interface TicketSelectionComment {
  author?: string | null | undefined;
  body: string;
}

export interface TicketSelectionContentInput {
  key: string;
  title?: string | null | undefined;
  status?: string | null | undefined;
  assignees?: readonly string[] | null | undefined;
  reporter?: string | null | undefined;
  description?: string | null | undefined;
  acceptanceCriteria?: string | null | undefined;
  comments?: readonly TicketSelectionComment[] | null | undefined;
}

export function buildTicketSelectionContent({
  key,
  title,
  status,
  assignees,
  reporter,
  description,
  acceptanceCriteria,
  comments,
}: TicketSelectionContentInput): string {
  const lines: string[] = [`# ${key}${title?.trim() ? `: ${title.trim()}` : ""}`];
  const metadata = [
    status?.trim() ? `Status: ${status.trim()}` : null,
    assignees && assignees.length > 0
      ? `Assignee${assignees.length === 1 ? "" : "s"}: ${assignees.join(", ")}`
      : null,
    reporter?.trim() ? `Reporter: ${reporter.trim()}` : null,
  ].filter((line): line is string => Boolean(line));
  if (metadata.length > 0) {
    lines.push("", ...metadata);
  }
  appendSection(lines, "Description", description);
  appendSection(lines, "Acceptance Criteria", acceptanceCriteria);
  if (comments && comments.length > 0) {
    lines.push("", "## Comments");
    for (const comment of comments) {
      if (!comment.body.trim()) continue;
      lines.push(
        "",
        `### ${comment.author?.trim() || "Comment"}`,
        "",
        comment.body.trim(),
      );
    }
  }
  return lines.join("\n");
}

function appendSection(
  lines: string[],
  heading: string,
  content: string | null | undefined,
): void {
  const normalized = content?.trim();
  if (!normalized) return;
  lines.push("", `## ${heading}`, "", normalized);
}
