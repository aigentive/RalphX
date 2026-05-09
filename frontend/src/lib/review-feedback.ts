const ANSI_ESCAPE_RE = new RegExp(
  `${String.fromCharCode(27)}\\[[0-9;?]*[ -/]*[@-~]`,
  "g"
);

export function sanitizeReviewFeedbackText(text: string): string {
  return text
    .replace(ANSI_ESCAPE_RE, "")
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .trim();
}

export function buildReviewFeedbackPreview(
  text: string,
  previewCharLimit = 900
): string {
  const sanitized = sanitizeReviewFeedbackText(text);
  if (sanitized.length <= previewCharLimit) {
    return sanitized;
  }
  // Slice at the last whitespace before the limit so we don't cut mid-word
  // or mid-markdown-token. Preserve the original newlines so markdown
  // (lists, paragraphs, code fences) keeps its structure.
  const cut = sanitized.slice(0, previewCharLimit);
  const lastWs = Math.max(cut.lastIndexOf("\n"), cut.lastIndexOf(" "));
  const safeEnd = lastWs > previewCharLimit * 0.7 ? lastWs : previewCharLimit;
  return `${sanitized.slice(0, safeEnd).trimEnd()}…`;
}

export function getReviewFeedbackHeading(
  reviewer: string,
  compact = false
): string {
  if (reviewer === "ai") {
    return compact ? "AI Feedback" : "AI Review Feedback";
  }
  if (reviewer === "system") {
    return compact ? "System Feedback" : "System Review Feedback";
  }
  return compact ? "Human Feedback" : "Human Review Feedback";
}

export function getReviewerActorLabel(reviewer: string): string {
  if (reviewer === "ai") {
    return "AI Reviewer";
  }
  if (reviewer === "system") {
    return "System Reviewer";
  }
  return "Human Reviewer";
}

export function getReviewerTypeLabel(reviewer: string): string {
  if (reviewer === "ai") {
    return "AI Review";
  }
  if (reviewer === "system") {
    return "System Review";
  }
  return "Human Review";
}
