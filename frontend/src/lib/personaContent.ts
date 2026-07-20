export interface ParsedPersonaDocument {
  name: string;
  kind: string;
  description: string;
  body: string;
}

function decodeFrontmatterScalar(value: string): string {
  const trimmed = value.trim();
  if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
    try {
      return JSON.parse(trimmed) as string;
    } catch {
      return trimmed.slice(1, -1);
    }
  }
  if (trimmed.startsWith("'") && trimmed.endsWith("'")) {
    return trimmed.slice(1, -1).split("''").join("'");
  }
  return trimmed;
}

/** Parses the canonical, single-line Persona YAML frontmatter and Markdown body. */
export function parsePersonaDocument(content: string): ParsedPersonaDocument | null {
  const lines = content.split(/\r?\n/);
  if (lines[0] !== "---") return null;

  const closingDelimiter = lines.findIndex(
    (line, index) => index > 0 && line === "---",
  );
  if (closingDelimiter === -1) return null;

  const fields = new Map<string, string>();
  for (const line of lines.slice(1, closingDelimiter)) {
    const separator = line.indexOf(":");
    if (separator <= 0) continue;
    const key = line.slice(0, separator).trim();
    const value = line.slice(separator + 1);
    if (key === "name" || key === "kind" || key === "description") {
      fields.set(key, decodeFrontmatterScalar(value));
    }
  }

  const name = fields.get("name");
  const kind = fields.get("kind");
  const description = fields.get("description");
  if (name === undefined || kind === undefined || description === undefined) {
    return null;
  }

  const body = lines
    .slice(closingDelimiter + 1)
    .join("\n")
    .replace(/^\n/, "");

  return { name, kind, description, body };
}

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
