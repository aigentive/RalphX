/**
 * ArtifactWidget — Artifact preview card for get_artifact, get_artifact_version,
 * get_related_artifacts, and search_project_artifacts.
 *
 * Single artifact: type badge + title + markdown preview (collapsible).
 * Artifact lists: count badge + list of artifact titles with type labels.
 * Collapsible via WidgetCard.
 */

import React from "react";
import { WidgetCard, Badge, InlineIndicator } from "./shared";
import { parseMcpToolResult, parseMcpToolResultRaw } from "./shared.constants";
import type { ToolCall } from "../ToolCallIndicator";

// ============================================================================
// Types
// ============================================================================

interface ParsedArtifact {
  title: string;
  artifactType: string;
  content: string;
  version?: number;
}

interface ArtifactListItem {
  title: string;
  artifactType: string;
  contentPreview?: string;
}

interface ArtifactWidgetProps {
  toolCall: ToolCall;
  compact?: boolean;
}

// ============================================================================
// Parsing
// ============================================================================

function parseArtifact(toolCall: ToolCall): ParsedArtifact | null {
  const { name } = toolCall;
  const r = parseMcpToolResult(toolCall.result);
  if (!r || Object.keys(r).length === 0) return null;

  // get_artifact: { id, title, artifact_type, content, content_preview, version }
  // get_artifact_version: same shape, with explicit version param in args
  const title =
    (typeof r.title === "string" ? r.title : null) ??
    (typeof r.name === "string" ? r.name : null) ??
    "Untitled Artifact";

  const artifactType =
    (typeof r.artifact_type === "string" ? r.artifact_type : null) ??
    (typeof r.artifactType === "string" ? r.artifactType : null) ??
    "unknown";

  const content =
    (typeof r.content === "string" ? r.content : null) ??
    (typeof r.content_preview === "string" ? r.content_preview : null) ??
    (typeof r.contentPreview === "string" ? r.contentPreview : null) ??
    "";

  // For get_artifact_version, extract version from args
  let version: number | undefined;
  if (name.toLowerCase().includes("get_artifact_version")) {
    const args = toolCall.arguments as Record<string, unknown> | undefined;
    if (args && typeof args.version === "number") {
      version = args.version;
    }
  } else if (typeof r.version === "number") {
    version = r.version;
  }

  const base = { title, artifactType, content };
  return version != null ? { ...base, version } : base;
}

// ============================================================================
// Lightweight Markdown Preview
// ============================================================================

/** Render first few lines of markdown as styled preview (headings bold, paragraphs secondary, code mono) */
function MarkdownPreview({ content, compact }: { content: string; compact?: boolean }) {
  const lines = content.split("\n").filter((l) => l.trim() !== "");
  // Show first ~5 non-empty lines for preview
  const previewLines = lines.slice(0, 5);

  return (
    <div className={`${compact ? "text-[0.6562rem]" : "text-[0.7188rem]"} leading-[1.55] py-1.5`}>
      {previewLines.map((line, i) => {
        const trimmed = line.trim();

        // H1: # heading
        if (trimmed.startsWith("# ")) {
          return (
            <div
              key={i}
              className={`${compact ? "text-xs" : "text-[0.8125rem]"} font-semibold mb-1`}
              style={{ color: "var(--text-primary)" }}
            >
              {trimmed.replace(/^#+\s*/, "")}
            </div>
          );
        }

        // H2: ## heading
        if (trimmed.startsWith("## ")) {
          return (
            <div
              key={i}
              className={`${compact ? "text-[0.6562rem]" : "text-[0.7188rem]"} font-semibold mt-1.5`}
              style={{ color: "var(--text-primary)" }}
            >
              {trimmed.replace(/^#+\s*/, "")}
            </div>
          );
        }

        // H3+: ### heading
        if (trimmed.startsWith("###")) {
          return (
            <div
              key={i}
              className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} font-semibold mt-1`}
              style={{ color: "var(--text-primary)" }}
            >
              {trimmed.replace(/^#+\s*/, "")}
            </div>
          );
        }

        // Code block markers (``` lines) — skip
        if (trimmed.startsWith("```")) {
          return null;
        }

        // Lines that look like code (indented or inside code blocks)
        if (line.startsWith("    ") || line.startsWith("\t")) {
          return (
            <div
              key={i}
              className={`${compact ? "text-[0.625rem]" : "text-[0.6562rem]"} mt-0.5`}
              style={{
                color: "var(--text-secondary)",
                fontFamily: "var(--font-mono)",
              }}
            >
              {trimmed}
            </div>
          );
        }

        // Regular paragraph
        return (
          <div
            key={i}
            className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} mt-0.5`}
            style={{ color: "var(--text-secondary)" }}
          >
            {trimmed}
          </div>
        );
      })}
    </div>
  );
}

// ============================================================================
// Parsing — Artifact lists (get_related_artifacts, search_project_artifacts)
// ============================================================================

function isArtifactListTool(name: string): boolean {
  const n = name.toLowerCase();
  return n.includes("get_related_artifacts") || n.includes("search_project_artifacts");
}

function parseArtifactList(toolCall: ToolCall): ArtifactListItem[] | null {
  const { result } = toolCall;
  if (!result) return null;

  // Use parseMcpToolResultRaw (not parseMcpToolResult) — returns unknown for Array.isArray narrowing
  const raw = parseMcpToolResultRaw(result);
  if (!Array.isArray(raw)) return null;

  return raw.map((item: Record<string, unknown>) => {
    const base: ArtifactListItem = {
      title: (typeof item.title === "string" ? item.title : null) ?? "Untitled",
      artifactType:
        (typeof item.artifact_type === "string" ? item.artifact_type : null) ??
        (typeof item.artifactType === "string" ? item.artifactType : null) ??
        "unknown",
    };
    const preview =
      (typeof item.content_preview === "string" ? item.content_preview : null) ??
      (typeof item.contentPreview === "string" ? item.contentPreview : null);
    if (preview) base.contentPreview = preview;
    return base;
  });
}

// ============================================================================
// Type Badge
// ============================================================================

const TYPE_LABELS: Record<string, string> = {
  specification: "SPEC",
  research: "RESEARCH",
  design_doc: "DESIGN",
  decision: "DECISION",
  test_plan: "TEST",
  code: "CODE",
};

function artifactTypeLabel(type: string): string {
  const lower = type.toLowerCase();
  return TYPE_LABELS[lower] ?? type.toUpperCase().slice(0, 8);
}

// ============================================================================
// Component
// ============================================================================

export const ArtifactWidget = React.memo(function ArtifactWidget({
  toolCall,
  compact = false,
}: ArtifactWidgetProps) {
  // Artifact list tools (get_related_artifacts, search_project_artifacts)
  if (isArtifactListTool(toolCall.name)) {
    return <ArtifactListView toolCall={toolCall} compact={compact} />;
  }

  // Single artifact tools (get_artifact, get_artifact_version)
  const parsed = parseArtifact(toolCall);
  if (!parsed) return null;

  const header = (
    <>
      <span
        className="text-[0.5625rem] px-1.5 py-px rounded font-semibold uppercase tracking-wider flex-shrink-0"
        style={{
          background: "var(--bg-hover)",
          color: "var(--text-muted)",
          letterSpacing: "0.04em",
        }}
      >
        {artifactTypeLabel(parsed.artifactType)}
      </span>
      <span
        className={`${compact ? "text-[0.6875rem]" : "text-[0.7188rem]"} font-medium flex-1 min-w-0 truncate`}
        style={{ color: "var(--text-secondary)" }}
      >
        {parsed.title}
      </span>
      {parsed.version != null && (
        <Badge variant="muted">v{parsed.version}</Badge>
      )}
    </>
  );

  return (
    <WidgetCard
      header={header}
      compact={compact}
      defaultExpanded={false}
    >
      {parsed.content ? (
        <MarkdownPreview content={parsed.content} compact={compact} />
      ) : (
        <div
          className={`${compact ? "text-[0.625rem]" : "text-[0.6562rem]"} py-1`}
          style={{ color: "var(--text-muted)" }}
        >
          No content preview available
        </div>
      )}
    </WidgetCard>
  );
});

// ============================================================================
// ArtifactListView — For get_related_artifacts / search_project_artifacts
// ============================================================================

function ArtifactListView({ toolCall, compact }: { toolCall: ToolCall; compact: boolean }) {
  const artifacts = parseArtifactList(toolCall);
  const isSearch = toolCall.name.toLowerCase().includes("search_project_artifacts");
  const query = isSearch ? (toolCall.arguments as { query?: string })?.query : undefined;

  if (!artifacts || artifacts.length === 0) {
    const label = isSearch ? "No artifacts found" : "No related artifacts";
    return <InlineIndicator text={label} />;
  }

  const headerTitle = isSearch
    ? (query ? `"${query.length > 30 ? query.slice(0, 30) + "..." : query}"` : "Artifact search")
    : "Related artifacts";

  const header = (
    <>
      <span
        className={`${compact ? "text-[0.6875rem]" : "text-[0.7188rem]"} font-medium flex-1 min-w-0 truncate`}
        style={{ color: "var(--text-secondary)" }}
      >
        {headerTitle}
      </span>
      <Badge variant="muted">
        {artifacts.length} result{artifacts.length !== 1 ? "s" : ""}
      </Badge>
    </>
  );

  return (
    <WidgetCard header={header} compact={compact} defaultExpanded={false}>
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {artifacts.slice(0, 5).map((artifact, idx) => (
          <div
            key={idx}
            style={{
              padding: compact ? "4px 6px" : "5px 8px",
              borderRadius: 6,
              background: "var(--bg-surface)",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <span
                style={{
                  fontSize: 9,
                  padding: "1px 4px",
                  borderRadius: 3,
                  fontWeight: 600,
                  textTransform: "uppercase",
                  letterSpacing: "0.04em",
                  background: "var(--bg-hover)",
                  color: "var(--text-muted)",
                  flexShrink: 0,
                }}
              >
                {artifactTypeLabel(artifact.artifactType)}
              </span>
              <span
                style={{
                  fontSize: compact ? 10.5 : 11,
                  fontWeight: 500,
                  color: "var(--text-secondary)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  flex: 1,
                }}
              >
                {artifact.title}
              </span>
            </div>
            {artifact.contentPreview && (
              <div
                style={{
                  fontSize: compact ? 9.5 : 10,
                  color: "var(--text-muted)",
                  marginTop: 2,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {artifact.contentPreview.length > 80
                  ? artifact.contentPreview.slice(0, 80) + "..."
                  : artifact.contentPreview}
              </div>
            )}
          </div>
        ))}
        {artifacts.length > 5 && (
          <div
            style={{
              fontSize: compact ? 9.5 : 10,
              color: "var(--text-muted)",
              padding: "2px 6px",
            }}
          >
            +{artifacts.length - 5} more
          </div>
        )}
      </div>
    </WidgetCard>
  );
}
