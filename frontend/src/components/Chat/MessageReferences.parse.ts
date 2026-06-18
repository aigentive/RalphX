import type {
  ComposerArtifactReference,
  ComposerIntegrationReference,
  ComposerProjectReference,
} from "@/api/chat";

export interface MessageComposerReferences {
  projectReferences: ComposerProjectReference[];
  integrationReferences: ComposerIntegrationReference[];
  artifactReferences: ComposerArtifactReference[];
}

export function serializeComposerReferencesMetadata({
  projectReferences,
  integrationReferences,
  artifactReferences,
}: {
  projectReferences?: ComposerProjectReference[] | null | undefined;
  integrationReferences?: ComposerIntegrationReference[] | null | undefined;
  artifactReferences?: ComposerArtifactReference[] | null | undefined;
}): string | null {
  const normalizedProjectReferences = parseProjectReferences(projectReferences);
  const normalizedIntegrationReferences =
    parseIntegrationReferences(integrationReferences);
  const normalizedArtifactReferences = parseArtifactReferences(artifactReferences);

  if (
    normalizedProjectReferences.length === 0 &&
    normalizedIntegrationReferences.length === 0 &&
    normalizedArtifactReferences.length === 0
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
    metadata.composer_integration_references ?? metadata.composerIntegrationReferences,
  );
  const artifactReferences = parseArtifactReferences(
    metadata.composer_artifact_references ?? metadata.composerArtifactReferences,
  );

  if (
    projectReferences.length === 0 &&
    integrationReferences.length === 0 &&
    artifactReferences.length === 0
  ) {
    return null;
  }

  return { projectReferences, integrationReferences, artifactReferences };
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

function parseIntegrationReferences(raw: unknown): ComposerIntegrationReference[] {
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
      record.provider !== "atlassian" ||
      (record.kind !== "jira" && record.kind !== "confluence") ||
      typeof record.id !== "string" ||
      record.id.trim().length === 0
    ) {
      continue;
    }

    references.push({
      provider: "atlassian",
      kind: record.kind,
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
    const kind = typeof record.kind === "string" && record.kind.trim() ? record.kind : "plan";
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
