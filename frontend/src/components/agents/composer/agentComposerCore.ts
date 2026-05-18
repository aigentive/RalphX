export type AgentComposerTriggerKind = "path" | "skill" | "slash-command";

export interface AgentComposerTrigger {
  kind: AgentComposerTriggerKind;
  query: string;
  rangeStart: number;
  rangeEnd: number;
}

export interface AgentComposerProjectReference {
  path: string;
  kind?: "file" | "directory";
}

const TOKEN_BOUNDARY_PATTERN = /\s/;

export function detectAgentComposerTrigger(
  text: string,
  cursor: number,
): AgentComposerTrigger | null {
  const safeCursor = Math.max(0, Math.min(cursor, text.length));
  const lineStart = text.lastIndexOf("\n", safeCursor - 1) + 1;
  const linePrefix = text.slice(lineStart, safeCursor);
  const slashMatch = /^\/(\S*)$/.exec(linePrefix);
  if (slashMatch) {
    return {
      kind: "slash-command",
      query: slashMatch[1] ?? "",
      rangeStart: lineStart,
      rangeEnd: safeCursor,
    };
  }

  const tokenStart = findCurrentTokenStart(text, safeCursor);
  const token = text.slice(tokenStart, safeCursor);
  const pathIndex = token.lastIndexOf("@");
  const skillIndex = token.lastIndexOf("$");
  const triggerIndex = Math.max(pathIndex, skillIndex);
  if (triggerIndex < 0) {
    return null;
  }

  const marker = token[triggerIndex];
  const rangeStart = tokenStart + triggerIndex;
  const query = text.slice(rangeStart + 1, safeCursor);
  if (query.includes("@") || query.includes("$")) {
    return null;
  }

  return {
    kind: marker === "@" ? "path" : "skill",
    query,
    rangeStart,
    rangeEnd: safeCursor,
  };
}

export function replaceAgentComposerTrigger(
  text: string,
  trigger: AgentComposerTrigger,
  replacement: string,
): { text: string; cursor: number } {
  const safeStart = Math.max(0, Math.min(trigger.rangeStart, text.length));
  let safeEnd = Math.max(safeStart, Math.min(trigger.rangeEnd, text.length));
  if (text[safeEnd] === " ") {
    safeEnd += 1;
  }
  const nextText = `${text.slice(0, safeStart)}${replacement}${text.slice(safeEnd)}`;
  return {
    text: nextText,
    cursor: safeStart + replacement.length,
  };
}

export function extractComposerSkillTokens(text: string): string[] {
  const names = new Set<string>();
  for (const match of text.matchAll(/\$([a-zA-Z0-9][a-zA-Z0-9_:-]*)/g)) {
    const name = match[1];
    if (name) {
      names.add(name);
    }
  }
  return [...names];
}

export function extractComposerPathTokens(text: string): AgentComposerProjectReference[] {
  const references = new Map<string, AgentComposerProjectReference>();
  for (const match of text.matchAll(/@([^\s]+)/g)) {
    const rawPath = match[1]?.replace(/[),.;:]+$/g, "");
    if (!rawPath || rawPath.includes("\0")) {
      continue;
    }
    references.set(rawPath, { path: rawPath });
  }
  return [...references.values()];
}

export function appendInternalSkillDirectives(
  text: string,
  skillNames: readonly string[],
): string {
  const safeNames = [...new Set(skillNames)]
    .map((name) => name.trim().toLowerCase())
    .filter((name) => /^[a-z0-9-]+$/.test(name));
  if (safeNames.length === 0) {
    return text;
  }
  const directives = safeNames
    .map((name) => `<!-- ralphx_internal_skill=${name} -->`)
    .join("\n");
  return `${text.trimEnd()}\n\n${directives}`;
}

export function normalizeComposerProjectReferences(
  references: readonly AgentComposerProjectReference[],
): AgentComposerProjectReference[] {
  const safeReferences = new Map<string, AgentComposerProjectReference>();
  for (const reference of references) {
    const path = reference.path.trim();
    if (!path || path.includes("\n") || path.includes("\r") || path.includes("\0")) {
      continue;
    }
    safeReferences.set(
      path,
      reference.kind ? { path, kind: reference.kind } : { path },
    );
  }
  return [...safeReferences.values()];
}

function findCurrentTokenStart(text: string, cursor: number): number {
  let index = cursor;
  while (index > 0 && !TOKEN_BOUNDARY_PATTERN.test(text[index - 1] ?? "")) {
    index -= 1;
  }
  return index;
}
