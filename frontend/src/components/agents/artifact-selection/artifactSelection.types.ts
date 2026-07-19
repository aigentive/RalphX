export const MAX_COMPOSER_EXCERPT_REFERENCES = 8;
export const MAX_COMPOSER_EXCERPT_BYTES = 16 * 1024;
export const MAX_COMPOSER_EXCERPT_AGGREGATE_BYTES = 64 * 1024;

export type ArtifactExcerptSourceKind =
  | "plan"
  | "review"
  | "issue"
  | "task"
  | "automation_spec"
  | "pull_request"
  | "workspace_diff"
  | "jira"
  | "linear"
  | "granola";

export interface ArtifactExcerptSource {
  sourceKind: ArtifactExcerptSourceKind;
  sourceId: string;
  sourceLabel: string;
  title?: string;
  artifactId?: string;
  sessionId?: string;
  version?: number;
  url?: string;
  filePath?: string;
  revision?: string;
  locator?: string;
}

export interface ComposerExcerptReference extends ArtifactExcerptSource {
  excerpt: string;
}

const encoder = new TextEncoder();

function safeText(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed && !trimmed.includes("\0") ? trimmed : undefined;
}

export function composerExcerptReferenceKey(
  reference: ComposerExcerptReference,
): string {
  return JSON.stringify([
    reference.sourceKind,
    reference.sourceId,
    reference.version ?? null,
    reference.revision ?? null,
    reference.locator ?? null,
    reference.excerpt,
  ]);
}

export function normalizeComposerExcerptReferences(
  references: readonly ComposerExcerptReference[],
): ComposerExcerptReference[] {
  const normalized = new Map<string, ComposerExcerptReference>();
  let aggregateBytes = 0;

  for (const reference of references) {
    if (normalized.size >= MAX_COMPOSER_EXCERPT_REFERENCES) break;

    const sourceId = safeText(reference.sourceId);
    const sourceLabel = safeText(reference.sourceLabel);
    const excerpt = reference.excerpt.trim();
    const excerptBytes = encoder.encode(excerpt).byteLength;
    if (
      !sourceId ||
      !sourceLabel ||
      !excerpt ||
      excerpt.includes("\0") ||
      excerptBytes > MAX_COMPOSER_EXCERPT_BYTES ||
      aggregateBytes + excerptBytes > MAX_COMPOSER_EXCERPT_AGGREGATE_BYTES
    ) {
      continue;
    }

    const version =
      typeof reference.version === "number" &&
      Number.isFinite(reference.version) &&
      reference.version >= 0
        ? reference.version
        : undefined;
    const title = safeText(reference.title);
    const artifactId = safeText(reference.artifactId);
    const sessionId = safeText(reference.sessionId);
    const url = safeText(reference.url);
    const filePath = safeText(reference.filePath);
    const revision = safeText(reference.revision);
    const locator = safeText(reference.locator);
    const next: ComposerExcerptReference = {
      sourceKind: reference.sourceKind,
      sourceId,
      sourceLabel,
      excerpt,
      ...(title ? { title } : {}),
      ...(artifactId ? { artifactId } : {}),
      ...(sessionId ? { sessionId } : {}),
      ...(version !== undefined ? { version } : {}),
      ...(url ? { url } : {}),
      ...(filePath ? { filePath } : {}),
      ...(revision ? { revision } : {}),
      ...(locator ? { locator } : {}),
    };
    const key = composerExcerptReferenceKey(next);
    if (normalized.has(key)) continue;
    normalized.set(key, next);
    aggregateBytes += excerptBytes;
  }

  return [...normalized.values()];
}
