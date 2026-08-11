export type AgentComposerTriggerKind =
  | "path"
  | "plan"
  | "skill"
  | "slash-command"
  | "integration";
export type AgentComposerIntegrationKind =
  | "jira"
  | "confluence"
  | "linear"
  | "clickup"
  | "granola";
export type AgentComposerIntegrationReferenceKind =
  | "jira"
  | "confluence"
  | "linear"
  | "clickup"
  | "note";
export type AgentComposerIntegrationProvider =
  | "atlassian"
  | "linear"
  | "clickup"
  | "granola";

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
  provider: AgentComposerIntegrationProvider;
  kind: AgentComposerIntegrationReferenceKind;
  id: string;
  key?: string;
  title?: string;
  url?: string;
  summaryExcerpt?: string;
  includeTranscript?: boolean;
  /**
   * User-selected excerpt text captured from an artifact surface (ticket, PR,
   * Granola note). Untrusted external context — never treated as instructions.
   */
  selectedExcerpt?: string;
  /** Human-readable location the excerpt was selected from (path/section). */
  selectedSourcePath?: string;
  /** Human-readable range/line label for the selection (e.g. "L12-L20"). */
  selectedRangeLabel?: string;
}

/** Bound on stored selected-excerpt length so composer metadata stays small. */
export const COMPOSER_SELECTED_EXCERPT_MAX_LENGTH = 4000;

export interface AgentComposerArtifactReference {
  artifactId: string;
  kind: "plan" | string;
  title?: string;
  sessionId?: string;
  version?: number;
  status?: string;
}

const TOKEN_BOUNDARY_PATTERN = /\s/;
const ATLASSIAN_URL_PATTERN = /https:\/\/[^\s<>"'`]+/gi;

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

  const lineIntegrationTrigger = detectIntegrationTriggerInLine(
    linePrefix,
    lineStart,
    safeCursor,
  );
  if (lineIntegrationTrigger) {
    return lineIntegrationTrigger;
  }

  const tokenStart = findCurrentTokenStart(text, safeCursor);
  const token = text.slice(tokenStart, safeCursor);
  if (token.startsWith("/")) {
    const query = token.slice(1);
    if (query.includes("/") || query.includes("@") || query.includes("$")) {
      return null;
    }
    return {
      kind: "skill",
      query,
      rangeStart: tokenStart,
      rangeEnd: safeCursor,
    };
  }

  const pathIndex = token.lastIndexOf("@");
  if (pathIndex < 0) {
    return null;
  }

  const rangeStart = tokenStart + pathIndex;
  const query = text.slice(rangeStart + 1, safeCursor);
  if (query.includes("@") || query.includes("$")) {
    return null;
  }
  const planTrigger = parsePlanTriggerQuery(query);
  if (planTrigger) {
    return {
      kind: "plan",
      query: planTrigger.query,
      rangeStart,
      rangeEnd: safeCursor,
    };
  }
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

  return {
    kind: "path",
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

export function extractComposerSlashSkillTokens(text: string): string[] {
  const names = new Set<string>();
  for (const match of text.matchAll(
    /(?:^|\s)\/([a-zA-Z0-9][a-zA-Z0-9_:-]*)(?=$|[\s),.;:!?])/g,
  )) {
    const name = match[1];
    if (name) {
      names.add(name);
    }
  }
  return [...names];
}

export function extractComposerPathTokens(
  text: string,
): AgentComposerProjectReference[] {
  const references = new Map<string, AgentComposerProjectReference>();
  for (const match of text.matchAll(/@([^\s]+)/g)) {
    const rawPath = match[1]?.replace(/[),.;:]+$/g, "");
    if (
      !rawPath ||
      rawPath.includes("\0") ||
      isIntegrationReferenceToken(rawPath) ||
      isPlanReferenceToken(rawPath)
    ) {
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
  for (const match of text.matchAll(
    /@(jira|confluence|conf|linear|clickup|granola):([^\s]+)/gi,
  )) {
    const rawKind = match[1]?.toLowerCase();
    const rawId = match[2]?.replace(/[),.;]+$/g, "");
    if (!rawKind || !rawId || rawId.includes("\0")) {
      continue;
    }
    const kind: AgentComposerIntegrationKind =
      rawKind === "jira"
        ? "jira"
        : rawKind === "linear"
          ? "linear"
          : rawKind === "clickup"
            ? "clickup"
            : rawKind === "granola"
              ? "granola"
              : "confluence";
    const id =
      kind === "jira" || kind === "linear" || kind === "clickup"
        ? rawId.toUpperCase()
        : rawId;
    const provider: AgentComposerIntegrationProvider =
      kind === "linear"
        ? "linear"
        : kind === "clickup"
          ? "clickup"
          : kind === "granola"
            ? "granola"
            : "atlassian";
    const reference: AgentComposerIntegrationReference = {
      provider,
      kind: kind === "granola" ? "note" : kind,
      id,
      ...(kind === "jira" || kind === "linear" || kind === "clickup"
        ? { key: id }
        : {}),
    };
    references.set(`${provider}:${kind}:${id}`, reference);
  }
  return [...references.values()];
}

export function extractComposerArtifactTokens(
  text: string,
): AgentComposerArtifactReference[] {
  const references = new Map<string, AgentComposerArtifactReference>();
  for (const match of text.matchAll(/@plan:([^\s]+)/gi)) {
    const rawId = match[1]?.replace(/[),.;]+$/g, "");
    if (!rawId || rawId.includes("\0")) {
      continue;
    }
    references.set(`plan:${rawId}`, {
      artifactId: rawId,
      kind: "plan",
    });
  }
  return [...references.values()];
}

export function extractPastedAtlassianResourceUrls(text: string): string[] {
  const urls = new Set<string>();
  for (const match of text.matchAll(ATLASSIAN_URL_PATTERN)) {
    const rawUrl = match[0].replace(/[),.;:!?]+$/g, "");
    let parsed: URL;
    try {
      parsed = new URL(rawUrl);
    } catch {
      continue;
    }
    if (
      parsed.protocol !== "https:" ||
      !isPlausibleAtlassianResourcePath(parsed.pathname)
    ) {
      continue;
    }
    urls.add(rawUrl);
  }
  return [...urls];
}

export function removeResolvedAtlassianResourceUrls(
  text: string,
  urls: readonly string[],
): string {
  let nextText = text;
  for (const url of [...urls].sort((a, b) => b.length - a.length)) {
    if (!url) {
      continue;
    }
    nextText = nextText.split(url).join("");
  }
  return nextText.replace(/[ \t]{2,}/g, " ");
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

export function normalizeComposerArtifactReferences(
  references: readonly AgentComposerArtifactReference[],
): AgentComposerArtifactReference[] {
  const safeReferences = new Map<string, AgentComposerArtifactReference>();
  for (const reference of references) {
    const artifactId = reference.artifactId.trim();
    const kind = reference.kind.trim() || "plan";
    if (
      !artifactId ||
      artifactId.includes("\n") ||
      artifactId.includes("\r") ||
      artifactId.includes("\0") ||
      kind.includes("\n") ||
      kind.includes("\r") ||
      kind.includes("\0")
    ) {
      continue;
    }
    const version =
      typeof reference.version === "number" &&
      Number.isFinite(reference.version)
        ? reference.version
        : undefined;
    const key = `${kind}:${artifactId}:${version ?? ""}`;
    if (safeReferences.has(key)) {
      continue;
    }
    safeReferences.set(key, {
      artifactId,
      kind,
      ...(reference.title?.trim() ? { title: reference.title.trim() } : {}),
      ...(reference.sessionId?.trim()
        ? { sessionId: reference.sessionId.trim() }
        : {}),
      ...(version !== undefined ? { version } : {}),
      ...(reference.status?.trim() ? { status: reference.status.trim() } : {}),
    });
  }
  return [...safeReferences.values()];
}

export function normalizeComposerProjectReferences(
  references: readonly AgentComposerProjectReference[],
): AgentComposerProjectReference[] {
  const safeReferences = new Map<string, AgentComposerProjectReference>();
  for (const reference of references) {
    const path = reference.path.trim();
    if (
      !path ||
      path.includes("\n") ||
      path.includes("\r") ||
      path.includes("\0")
    ) {
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
    const provider = reference.provider;
    const id = reference.id.trim();
    if (
      !id ||
      id.includes("\n") ||
      id.includes("\r") ||
      id.includes("\0") ||
      (provider === "atlassian" &&
        reference.kind !== "jira" &&
        reference.kind !== "confluence") ||
      (provider === "linear" && reference.kind !== "linear") ||
      (provider === "clickup" && reference.kind !== "clickup") ||
      (provider === "granola" && reference.kind !== "note") ||
      (provider !== "atlassian" &&
        provider !== "linear" &&
        provider !== "clickup" &&
        provider !== "granola")
    ) {
      continue;
    }
    const key =
      reference.kind === "jira" ||
      reference.kind === "linear" ||
      reference.kind === "clickup"
        ? (reference.key ?? id).trim()
        : undefined;
    safeReferences.set(`${provider}:${reference.kind}:${id}`, {
      provider,
      kind: reference.kind,
      id,
      ...(key ? { key } : {}),
      ...(reference.title ? { title: reference.title.trim() } : {}),
      ...(reference.url ? { url: reference.url.trim() } : {}),
      ...(reference.summaryExcerpt?.trim()
        ? { summaryExcerpt: reference.summaryExcerpt.trim() }
        : {}),
      ...(typeof reference.includeTranscript === "boolean"
        ? { includeTranscript: reference.includeTranscript }
        : {}),
      ...(normalizeSelectedExcerpt(reference.selectedExcerpt)
        ? { selectedExcerpt: normalizeSelectedExcerpt(reference.selectedExcerpt) }
        : {}),
      ...(reference.selectedSourcePath?.trim()
        ? { selectedSourcePath: reference.selectedSourcePath.trim().slice(0, 300) }
        : {}),
      ...(reference.selectedRangeLabel?.trim()
        ? { selectedRangeLabel: reference.selectedRangeLabel.trim().slice(0, 80) }
        : {}),
    });
  }
  return [...safeReferences.values()];
}

function normalizeSelectedExcerpt(
  excerpt: string | undefined,
): string | undefined {
  const trimmed = excerpt?.trim();
  if (!trimmed) {
    return undefined;
  }
  return trimmed.slice(0, COMPOSER_SELECTED_EXCERPT_MAX_LENGTH);
}

function parseIntegrationTriggerQuery(
  query: string,
): { kind: AgentComposerIntegrationKind; query: string } | null {
  const match = /^(jira|confluence|conf|linear|clickup|granola):(.*)$/i.exec(query);
  if (!match) {
    return null;
  }
  const rawKind = match[1]?.toLowerCase();
  const kind =
    rawKind === "jira"
      ? "jira"
      : rawKind === "linear"
        ? "linear"
        : rawKind === "clickup"
          ? "clickup"
          : rawKind === "granola"
            ? "granola"
            : "confluence";
  return { kind, query: match[2] ?? "" };
}

function parsePlanTriggerQuery(query: string): { query: string } | null {
  const match = /^plan:(.*)$/i.exec(query);
  if (!match) {
    return null;
  }
  return { query: match[1] ?? "" };
}

function isIntegrationReferenceToken(token: string): boolean {
  return /^(jira|confluence|conf|linear|clickup|granola):/i.test(token);
}

function isPlanReferenceToken(token: string): boolean {
  return /^plan:/i.test(token);
}

function isPlausibleAtlassianResourcePath(pathname: string): boolean {
  return (
    pathname.includes("/browse/") ||
    pathname.includes("/issues/") ||
    pathname === "/wiki" ||
    pathname.startsWith("/wiki/")
  );
}

function detectIntegrationTriggerInLine(
  linePrefix: string,
  lineStart: number,
  safeCursor: number,
): AgentComposerTrigger | null {
  const triggerPattern =
    /(^|[\s([{`'"])@(jira|confluence|conf|linear|clickup|granola):/gi;
  let lastMatch: {
    markerIndex: number;
    rawKind: string;
  } | null = null;

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
      lastMatch.rawKind.toLowerCase() === "jira"
        ? "jira"
        : lastMatch.rawKind.toLowerCase() === "linear"
          ? "linear"
          : lastMatch.rawKind.toLowerCase() === "clickup"
            ? "clickup"
            : lastMatch.rawKind.toLowerCase() === "granola"
              ? "granola"
              : "confluence",
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
