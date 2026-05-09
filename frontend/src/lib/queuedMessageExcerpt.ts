const DEFAULT_QUEUED_MESSAGE_EXCERPT_LENGTH = 240;
const ELLIPSIS = "...";

export function formatQueuedMessageExcerpt(
  content: string,
  maxLength = DEFAULT_QUEUED_MESSAGE_EXCERPT_LENGTH
): string {
  const compactContent = content.replace(/\s+/g, " ").trim();
  if (compactContent.length <= maxLength) {
    return compactContent;
  }

  const excerptLength = Math.max(0, maxLength - ELLIPSIS.length);
  return `${compactContent.slice(0, excerptLength).trimEnd()}${ELLIPSIS}`;
}
