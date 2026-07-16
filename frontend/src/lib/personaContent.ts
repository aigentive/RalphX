/** Returns the editable Markdown body from a canonical persona document. */
export function splitPersonaBody(content: string): string {
  const lines = content.split(/\r?\n/);
  if (lines[0] !== "---") return content;

  const closingDelimiter = lines.findIndex(
    (line, index) => index > 0 && line === "---",
  );
  if (closingDelimiter === -1) return content;

  return lines
    .slice(closingDelimiter + 1)
    .join("\n")
    .replace(/^\n/, "");
}

/** Composes the canonical persona document required by the draft-update command. */
export function composePersonaContent(
  slug: string,
  description: string,
  body: string,
): string {
  const normalizedDescription = description.trim().split(/\s+/).join(" ");
  return [
    "---",
    `name: ${slug}`,
    "kind: persona",
    `description: ${JSON.stringify(normalizedDescription)}`,
    "---",
    "",
    body.trim(),
    "",
  ].join("\n");
}
