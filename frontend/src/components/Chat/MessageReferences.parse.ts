import type {
  ComposerArtifactReference,
  ComposerExcerptReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  ComposerSelectionSnapshot,
} from "@/api/chat";

export interface MessageComposerReferences {
  folderReferences?: MessageFolderReference[];
  projectReferences: ComposerProjectReference[];
  integrationReferences: ComposerIntegrationReference[];
  artifactReferences: ComposerArtifactReference[];
  selectionSnapshot?: ComposerSelectionSnapshot;
  excerptReferences?: ComposerExcerptReference[];
}

export interface MessageFolderReference {
  id?: string;
  folderPath: string;
  displayName: string;
}

export function serializeComposerReferencesMetadata({
  folderReferences,
  projectReferences,
  integrationReferences,
  artifactReferences,
  selectionSnapshot,
  excerptReferences,
}: {
  folderReferences?: MessageFolderReference[] | null | undefined;
  projectReferences?: ComposerProjectReference[] | null | undefined;
  integrationReferences?: ComposerIntegrationReference[] | null | undefined;
  artifactReferences?: ComposerArtifactReference[] | null | undefined;
  selectionSnapshot?: ComposerSelectionSnapshot | null | undefined;
  excerptReferences?: ComposerExcerptReference[] | null | undefined;
}): string | null {
  const normalizedFolderReferences = parseFolderReferences(folderReferences);
  const normalizedProjectReferences = parseProjectReferences(projectReferences);
  const normalizedIntegrationReferences = parseIntegrationReferences(
    integrationReferences,
  );
  const normalizedArtifactReferences =
    parseArtifactReferences(artifactReferences);
  const normalizedSelectionSnapshot = parseSelectionSnapshot(selectionSnapshot);
  const normalizedExcerptReferences = parseExcerptReferences(excerptReferences);

  if (
    normalizedFolderReferences.length === 0 &&
    normalizedProjectReferences.length === 0 &&
    normalizedIntegrationReferences.length === 0 &&
    normalizedArtifactReferences.length === 0 &&
    !normalizedSelectionSnapshot &&
    normalizedExcerptReferences.length === 0
  ) {
    return null;
  }

  return JSON.stringify({
    ...(normalizedFolderReferences.length > 0
      ? { composer_folder_references: normalizedFolderReferences }
      : {}),
    ...(normalizedProjectReferences.length > 0
      ? { composer_project_references: normalizedProjectReferences }
      : {}),
    ...(normalizedIntegrationReferences.length > 0
      ? { composer_integration_references: normalizedIntegrationReferences }
      : {}),
    ...(normalizedArtifactReferences.length > 0
      ? { composer_artifact_references: normalizedArtifactReferences }
      : {}),
    ...(normalizedSelectionSnapshot
      ? { composer_selection_snapshot: normalizedSelectionSnapshot }
      : {}),
    ...(normalizedExcerptReferences.length > 0
      ? { composer_excerpt_references: normalizedExcerptReferences }
      : {}),
  });
}

export function parseComposerReferencesFromMetadata(
  metadata: Record<string, unknown> | null,
): MessageComposerReferences | null {
  if (!metadata) {
    return null;
  }

  const folderReferences = parseFolderReferences(
    metadata.composer_folder_references ?? metadata.composerFolderReferences,
  );
  const projectReferences = parseProjectReferences(
    metadata.composer_project_references ?? metadata.composerProjectReferences,
  );
  const integrationReferences = parseIntegrationReferences(
    metadata.composer_integration_references ??
      metadata.composerIntegrationReferences,
  );
  const artifactReferences = parseArtifactReferences(
    metadata.composer_artifact_references ??
      metadata.composerArtifactReferences,
  );
  const selectionSnapshot = parseSelectionSnapshot(
    metadata.composer_selection_snapshot ?? metadata.composerSelectionSnapshot,
  );
  const excerptReferences = parseExcerptReferences(
    metadata.composer_excerpt_references ?? metadata.composerExcerptReferences,
  );

  if (
    folderReferences.length === 0 &&
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0 &&
    !selectionSnapshot &&
    excerptReferences.length === 0
  ) {
    return null;
  }

  return {
    ...(folderReferences.length > 0 ? { folderReferences } : {}),
    projectReferences,
    integrationReferences,
    artifactReferences,
    ...(selectionSnapshot ? { selectionSnapshot } : {}),
    ...(excerptReferences.length > 0 ? { excerptReferences } : {}),
  };
}

function parseFolderReferences(raw: unknown): MessageFolderReference[] {
  if (!Array.isArray(raw)) return [];
  const references: MessageFolderReference[] = [];
  const seen = new Set<string>();
  for (const item of raw) {
    if (references.length >= 6) break;
    if (!item || typeof item !== "object") continue;
    const record = item as Record<string, unknown>;
    const folderPath = record.folderPath ?? record.folder_path;
    const displayName = record.displayName ?? record.display_name;
    const id = record.id;
    if (
      typeof folderPath !== "string" ||
      !folderPath.trim() ||
      folderPath.includes("\0") ||
      typeof displayName !== "string" ||
      !displayName.trim() ||
      displayName.includes("\0")
    ) {
      continue;
    }
    const key = typeof id === "string" && id.trim() ? id : folderPath;
    if (seen.has(key)) continue;
    seen.add(key);
    references.push({
      ...(typeof id === "string" && id.trim() ? { id } : {}),
      folderPath,
      displayName,
    });
  }
  return references;
}

const EXCERPT_SOURCE_KINDS = new Set([
  "plan",
  "review",
  "issue",
  "task",
  "automation_spec",
  "pull_request",
  "workspace_diff",
  "jira",
  "linear",
  "granola",
]);

function parseExcerptReferences(raw: unknown): ComposerExcerptReference[] {
  if (!Array.isArray(raw)) return [];
  const references: ComposerExcerptReference[] = [];
  let aggregateBytes = 0;
  for (const item of raw) {
    if (references.length >= 8) break;
    if (!item || typeof item !== "object") continue;
    const record = item as Record<string, unknown>;
    const sourceKind = record.sourceKind ?? record.source_kind;
    const sourceId = record.sourceId ?? record.source_id;
    const sourceLabel = record.sourceLabel ?? record.source_label;
    if (
      typeof sourceKind !== "string" ||
      !EXCERPT_SOURCE_KINDS.has(sourceKind) ||
      typeof sourceId !== "string" ||
      !sourceId.trim() ||
      typeof sourceLabel !== "string" ||
      !sourceLabel.trim() ||
      typeof record.excerpt !== "string" ||
      !record.excerpt.trim()
    ) {
      continue;
    }
    const excerptBytes = new TextEncoder().encode(record.excerpt).byteLength;
    if (excerptBytes > 16 * 1024 || aggregateBytes + excerptBytes > 64 * 1024) {
      continue;
    }
    const optionalString = (camel: string, snake: string) => {
      const value = record[camel] ?? record[snake];
      return typeof value === "string" && value.trim() ? value : undefined;
    };
    const title = optionalString("title", "title");
    const artifactId = optionalString("artifactId", "artifact_id");
    const sessionId = optionalString("sessionId", "session_id");
    const url = optionalString("url", "url");
    const filePath = optionalString("filePath", "file_path");
    const revision = optionalString("revision", "revision");
    const locator = optionalString("locator", "locator");
    references.push({
      sourceKind: sourceKind as ComposerExcerptReference["sourceKind"],
      sourceId,
      sourceLabel,
      excerpt: record.excerpt,
      ...(title ? { title } : {}),
      ...(artifactId ? { artifactId } : {}),
      ...(sessionId ? { sessionId } : {}),
      ...(typeof record.version === "number" && Number.isFinite(record.version)
        ? { version: record.version }
        : {}),
      ...(url ? { url } : {}),
      ...(filePath ? { filePath } : {}),
      ...(revision ? { revision } : {}),
      ...(locator ? { locator } : {}),
    });
    aggregateBytes += excerptBytes;
  }
  return references;
}

function parseSelectionSnapshot(raw: unknown): ComposerSelectionSnapshot | null {
  if (!raw || typeof raw !== "object") {
    return null;
  }
  const record = raw as Record<string, unknown>;
  const sourceType = readString(record, "sourceType", "source_type");
  const sourceKind = readString(record, "sourceKind", "source_kind");
  const sourceId = readString(record, "sourceId", "source_id");
  const startLine = readNumber(record, "startLine", "start_line");
  const endLine = readNumber(record, "endLine", "end_line");
  const content = record.content;
  const sourcePairSupported =
    (sourceType === "artifact" && sourceKind === "plan") ||
    (sourceType === "note" && sourceKind === "granola") ||
    (sourceType === "ticket" &&
      (sourceKind === "jira" ||
        sourceKind === "linear" ||
        sourceKind === "clickup"));
  if (
    !sourcePairSupported ||
    !sourceId?.trim() ||
    !Number.isInteger(startLine) ||
    !Number.isInteger(endLine) ||
    !startLine ||
    !endLine ||
    startLine < 1 ||
    endLine < startLine ||
    typeof content !== "string" ||
    content.includes("\0") ||
    content.includes("\r") ||
    content.endsWith("\n") ||
    content.split("\n").length !== endLine - startLine + 1 ||
    new TextEncoder().encode(content).byteLength > 64 * 1024
  ) {
    return null;
  }

  const sourceTitle = readString(record, "sourceTitle", "source_title");
  const sourceKey = readString(record, "sourceKey", "source_key");
  const provider = readString(record, "provider", "provider");
  const artifactVersion = readNumber(
    record,
    "artifactVersion",
    "artifact_version",
  );
  const sourceRevision = readString(
    record,
    "sourceRevision",
    "source_revision",
  );
  const supportedProvider =
    (sourceKind === "jira" && provider === "atlassian") ||
    (sourceKind === "linear" && provider === "linear") ||
    (sourceKind === "clickup" && provider === "clickup") ||
    (sourceKind === "granola" && provider === "granola")
      ? provider
      : undefined;
  if (provider !== undefined && supportedProvider === undefined) {
    return null;
  }

  return {
    sourceType,
    sourceKind,
    sourceId,
    ...(sourceTitle?.trim() ? { sourceTitle } : {}),
    ...(sourceKey?.trim() ? { sourceKey } : {}),
    ...(supportedProvider ? { provider: supportedProvider } : {}),
    ...(artifactVersion && Number.isInteger(artifactVersion) && artifactVersion > 0
      ? { artifactVersion }
      : {}),
    ...(sourceRevision?.trim() ? { sourceRevision } : {}),
    startLine,
    endLine,
    content,
  };
}

function readString(
  record: Record<string, unknown>,
  camelKey: string,
  snakeKey: string,
): string | undefined {
  const value = record[camelKey] ?? record[snakeKey];
  return typeof value === "string" ? value : undefined;
}

function readNumber(
  record: Record<string, unknown>,
  camelKey: string,
  snakeKey: string,
): number | undefined {
  const value = record[camelKey] ?? record[snakeKey];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function parseProjectReferences(raw: unknown): ComposerProjectReference[] {
  if (!Array.isArray(raw)) {
    return [];
  }

  const references: ComposerProjectReference[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") {
      continue;
    }
    const record = item as Record<string, unknown>;
    if (typeof record.path !== "string" || record.path.trim().length === 0) {
      continue;
    }
    const kind =
      record.kind === "file" || record.kind === "directory"
        ? record.kind
        : undefined;
    references.push({
      path: record.path,
      ...(kind ? { kind } : {}),
    });
  }
  return references;
}

function parseIntegrationReferences(
  raw: unknown,
): ComposerIntegrationReference[] {
  if (!Array.isArray(raw)) {
    return [];
  }

  const references: ComposerIntegrationReference[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") {
      continue;
    }
    const record = item as Record<string, unknown>;
    if (
      (record.provider !== "atlassian" &&
        record.provider !== "linear" &&
        record.provider !== "clickup" &&
        record.provider !== "granola") ||
      (record.provider === "atlassian" &&
        record.kind !== "jira" &&
        record.kind !== "jira_board" &&
        record.kind !== "confluence" &&
        record.kind !== "confluence_link") ||
      (record.provider === "linear" && record.kind !== "linear") ||
      (record.provider === "clickup" && record.kind !== "clickup") ||
      (record.provider === "granola" && record.kind !== "note") ||
      typeof record.id !== "string" ||
      record.id.trim().length === 0
    ) {
      continue;
    }

    const provider = record.provider as
      | "atlassian"
      | "linear"
      | "clickup"
      | "granola";
    const kind = record.kind as
      | "jira"
      | "jira_board"
      | "confluence"
      | "confluence_link"
      | "linear"
      | "clickup"
      | "note";
    references.push({
      provider,
      kind,
      id: record.id,
      ...(typeof record.key === "string" && record.key.trim().length > 0
        ? { key: record.key }
        : {}),
      ...(typeof record.title === "string" && record.title.trim().length > 0
        ? { title: record.title }
        : {}),
      ...(typeof record.url === "string" && record.url.trim().length > 0
        ? { url: record.url }
        : {}),
      ...(typeof record.summaryExcerpt === "string" &&
      record.summaryExcerpt.trim().length > 0
        ? { summaryExcerpt: record.summaryExcerpt }
        : {}),
      ...(typeof record.summary_excerpt === "string" &&
      record.summary_excerpt.trim().length > 0
        ? { summaryExcerpt: record.summary_excerpt }
        : {}),
      ...(typeof record.includeTranscript === "boolean"
        ? { includeTranscript: record.includeTranscript }
        : {}),
      ...(typeof record.include_transcript === "boolean"
        ? { includeTranscript: record.include_transcript }
        : {}),
    });
  }
  return references;
}

function parseArtifactReferences(raw: unknown): ComposerArtifactReference[] {
  if (!Array.isArray(raw)) {
    return [];
  }

  const references: ComposerArtifactReference[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") {
      continue;
    }
    const record = item as Record<string, unknown>;
    const artifactId =
      typeof record.artifactId === "string"
        ? record.artifactId
        : typeof record.artifact_id === "string"
          ? record.artifact_id
          : null;
    if (!artifactId || artifactId.trim().length === 0) {
      continue;
    }
    const kind =
      typeof record.kind === "string" && record.kind.trim()
        ? record.kind
        : "plan";
    const sessionId =
      typeof record.sessionId === "string"
        ? record.sessionId
        : typeof record.session_id === "string"
          ? record.session_id
          : undefined;
    const version =
      typeof record.version === "number" && Number.isFinite(record.version)
        ? record.version
        : undefined;
    references.push({
      artifactId,
      kind,
      ...(typeof record.title === "string" && record.title.trim().length > 0
        ? { title: record.title }
        : {}),
      ...(sessionId && sessionId.trim().length > 0 ? { sessionId } : {}),
      ...(version !== undefined ? { version } : {}),
      ...(typeof record.status === "string" && record.status.trim().length > 0
        ? { status: record.status }
        : {}),
    });
  }
  return references;
}
