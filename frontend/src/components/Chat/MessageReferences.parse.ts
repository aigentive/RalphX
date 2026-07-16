import type {
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
  ComposerSelectionSnapshot,
} from "@/api/chat";

export interface MessageComposerReferences {
  projectReferences: ComposerProjectReference[];
  integrationReferences: ComposerIntegrationReference[];
  artifactReferences: ComposerArtifactReference[];
  selectionSnapshot?: ComposerSelectionSnapshot;
}

export function serializeComposerReferencesMetadata({
  projectReferences,
  integrationReferences,
  artifactReferences,
  selectionSnapshot,
}: {
  projectReferences?: ComposerProjectReference[] | null | undefined;
  integrationReferences?: ComposerIntegrationReference[] | null | undefined;
  artifactReferences?: ComposerArtifactReference[] | null | undefined;
  selectionSnapshot?: ComposerSelectionSnapshot | null | undefined;
}): string | null {
  const normalizedProjectReferences = parseProjectReferences(projectReferences);
  const normalizedIntegrationReferences = parseIntegrationReferences(
    integrationReferences,
  );
  const normalizedArtifactReferences =
    parseArtifactReferences(artifactReferences);
  const normalizedSelectionSnapshot = parseSelectionSnapshot(selectionSnapshot);

  if (
    normalizedProjectReferences.length === 0 &&
    normalizedIntegrationReferences.length === 0 &&
    normalizedArtifactReferences.length === 0 &&
    !normalizedSelectionSnapshot
  ) {
    return null;
  }

  return JSON.stringify({
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
  });
}

export function parseComposerReferencesFromMetadata(
  metadata: Record<string, unknown> | null,
): MessageComposerReferences | null {
  if (!metadata) {
    return null;
  }

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

  if (
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0 &&
    !selectionSnapshot
  ) {
    return null;
  }

  return {
    projectReferences,
    integrationReferences,
    artifactReferences,
    ...(selectionSnapshot ? { selectionSnapshot } : {}),
  };
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
        record.kind !== "confluence") ||
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
      | "confluence"
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
