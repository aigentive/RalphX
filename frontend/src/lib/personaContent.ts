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
