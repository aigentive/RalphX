/**
 * Proposal text helpers
 *
 * Some proposal fields (description, implementation steps, acceptance criteria)
 * are persisted as JSON-encoded strings without their outer quotes, so they
 * arrive on the client with literal `\n` and `\"` escape sequences instead of
 * real newlines and quotes. Decode the standard escapes when we detect them
 * (and the text has no real newlines yet) so markdown rendering can pick up
 * paragraph breaks, lists, and quoted code spans correctly.
 */
export function unescapeProposalText(text: string): string {
  if (!text) return text;
  const hasEscapedNewline = /\\n/.test(text);
  const hasRealNewline = /\n/.test(text);
  const hasEscapedQuote = /\\"/.test(text);
  if (hasRealNewline || (!hasEscapedNewline && !hasEscapedQuote)) {
    return text;
  }
  return text
    .replace(/\\n/g, "\n")
    .replace(/\\r/g, "\r")
    .replace(/\\t/g, "\t")
    .replace(/\\"/g, '"')
    .replace(/\\\\/g, "\\");
}

/**
 * Single-line preview: unescape + replace newlines with spaces so a clamped
 * card preview reads as a sentence rather than fragments.
 */
export function buildProposalPreview(text: string): string {
  return unescapeProposalText(text).replace(/\s+/g, " ").trim();
}
