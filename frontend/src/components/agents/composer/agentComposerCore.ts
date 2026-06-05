export type AgentComposerTriggerKind =
  | "path"
  | "skill"
  | "slash-command"
  | "integration";
export type AgentComposerIntegrationKind = "jira" | "confluence";

export interface AgentComposerTrigger {
  kind: AgentComposerTriggerKind;
  query: string;
  rangeStart: number;
  rangeEnd: number;
  integrationKind?: AgentComposerIntegrationKind;
}

export interface AgentComposerProjectReference {
  path: string;
  kind?: "file" | "directory";
}

export interface AgentComposerIntegrationReference {
  provider: "atlassian";
  kind: AgentComposerIntegrationKind;
  id: string;
  key?: string;
  title?: string;
  url?: string;
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

  const integrationTrigger = detectIntegrationTriggerInLine(
    linePrefix,
    lineStart,
    safeCursor,
  );
  if (integrationTrigger) {
    return integrationTrigger;
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
  if (marker === "@") {
    const integrationTrigger = parseIntegrationTriggerQuery(query);
    if (integrationTrigger) {
      return {
        kind: "integration",
        query: integrationTrigger.query,
        integrationKind: integrationTrigger.kind,
        rangeStart,
        rangeEnd: safeCursor,
      };
    }
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
    if (!rawPath || rawPath.includes("\0") || isIntegrationReferenceToken(rawPath)) {
      continue;
    }
    references.set(rawPath, { path: rawPath });
  }
  return [...references.values()];
}

export function extractComposerIntegrationTokens(
  text: string,
): AgentComposerIntegrationReference[] {
  const references = new Map<string, AgentComposerIntegrationReference>();
  for (const match of text.matchAll(/@(jira|confluence|conf):([^\s]+)/gi)) {
    const rawKind = match[1]?.toLowerCase();
    const rawId = match[2]?.replace(/[),.;]+$/g, "");
    if (!rawKind || !rawId || rawId.includes("\0")) {
      continue;
    }
    const kind: AgentComposerIntegrationKind =
      rawKind === "jira" ? "jira" : "confluence";
    const id = kind === "jira" ? rawId.toUpperCase() : rawId;
    const reference: AgentComposerIntegrationReference = {
      provider: "atlassian",
      kind,
      id,
      ...(kind === "jira" ? { key: id } : {}),
    };
    references.set(`${kind}:${id}`, reference);
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

export function normalizeComposerIntegrationReferences(
  references: readonly AgentComposerIntegrationReference[],
): AgentComposerIntegrationReference[] {
  const safeReferences = new Map<string, AgentComposerIntegrationReference>();
  for (const reference of references) {
    if (reference.provider !== "atlassian") {
      continue;
    }
    const id = reference.id.trim();
    if (
      !id ||
      id.includes("\n") ||
      id.includes("\r") ||
      id.includes("\0") ||
      (reference.kind !== "jira" && reference.kind !== "confluence")
    ) {
      continue;
    }
    const key = reference.kind === "jira" ? (reference.key ?? id).trim() : undefined;
    safeReferences.set(`${reference.kind}:${id}`, {
      provider: "atlassian",
      kind: reference.kind,
      id,
      ...(key ? { key } : {}),
      ...(reference.title ? { title: reference.title.trim() } : {}),
      ...(reference.url ? { url: reference.url.trim() } : {}),
    });
  }
  return [...safeReferences.values()];
}

function parseIntegrationTriggerQuery(
  query: string,
): { kind: AgentComposerIntegrationKind; query: string } | null {
  const match = /^(jira|confluence|conf):(.*)$/i.exec(query);
  if (!match) {
    return null;
  }
  const kind = match[1]?.toLowerCase() === "jira" ? "jira" : "confluence";
  return { kind, query: match[2] ?? "" };
}

function isIntegrationReferenceToken(token: string): boolean {
  return /^(jira|confluence|conf):/i.test(token);
}

function detectIntegrationTriggerInLine(
  linePrefix: string,
  lineStart: number,
  safeCursor: number,
): AgentComposerTrigger | null {
  const triggerPattern = /(^|[\s([{`'"])@(jira|confluence|conf):/gi;
  let lastMatch:
    | {
        markerIndex: number;
        rawKind: string;
      }
    | null = null;

  for (const match of linePrefix.matchAll(triggerPattern)) {
    const boundary = match[1] ?? "";
    const rawKind = match[2];
    if (!rawKind || match.index === undefined) {
      continue;
    }
    lastMatch = {
      markerIndex: match.index + boundary.length,
      rawKind,
    };
  }

  if (!lastMatch) {
    return null;
  }

  const rangeStart = lineStart + lastMatch.markerIndex;
  const queryStart = lastMatch.markerIndex + `@${lastMatch.rawKind}:`.length;
  const query = linePrefix.slice(queryStart);
  if (query.includes("@") || query.includes("$")) {
    return null;
  }

  return {
    kind: "integration",
    integrationKind:
      lastMatch.rawKind.toLowerCase() === "jira" ? "jira" : "confluence",
    query,
    rangeStart,
    rangeEnd: safeCursor,
  };
}

function findCurrentTokenStart(text: string, cursor: number): number {
  let index = cursor;
  while (index > 0 && !TOKEN_BOUNDARY_PATTERN.test(text[index - 1] ?? "")) {
    index -= 1;
  }
  return index;
}
