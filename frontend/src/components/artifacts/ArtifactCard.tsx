/**
 * ArtifactCard - Displays an artifact with type badge and metadata
 *
 * Features:
 * - Artifact name and type badge with category coloring
 * - Created timestamp
 * - Version display (when > 1)
 * - Content type indicator (inline/file)
 * - Click handling for selection
 */

import type { Artifact, ArtifactType } from "@/types/artifact";
import {
  isDocumentArtifact,
  isCodeArtifact,
  isProcessArtifact,
  isContextArtifact,
  isLogArtifact,
} from "@/types/artifact";

// ============================================================================
// Types
// ============================================================================

interface ArtifactCardProps {
  artifact: Artifact;
  onClick: (artifactId: string) => void;
  isSelected?: boolean;
  disabled?: boolean;
}

type ArtifactCategory = "document" | "code" | "process" | "context" | "log";

// ============================================================================
// Helpers
// ============================================================================

const TYPE_LABELS: Record<ArtifactType, string> = {
  prd: "PRD",
  research_document: "Research Document",
  design_doc: "Design Doc",
  specification: "Specification",
  code_change: "Code Change",
  diff: "Diff",
  test_result: "Test Result",
  task_spec: "Task Spec",
  review_feedback: "Review Feedback",
  pr_review: "PR Review",
  approval: "Approval",
  findings: "Findings",
  recommendations: "Recommendations",
  context: "Context",
  previous_work: "Previous Work",
  research_brief: "Research Brief",
  activity_log: "Activity Log",
  alert: "Alert",
  intervention: "Intervention",
  persona: "Persona",
};

function getCategory(type: ArtifactType): ArtifactCategory {
  if (isDocumentArtifact(type)) return "document";
  if (isCodeArtifact(type)) return "code";
  if (isProcessArtifact(type)) return "process";
  if (isContextArtifact(type)) return "context";
  if (isLogArtifact(type)) return "log";
  return "document";
}

function formatTimestamp(iso: string): string {
  const date = new Date(iso);
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

// ============================================================================
// Component
// ============================================================================

export function ArtifactCard({ artifact, onClick, isSelected = false, disabled = false }: ArtifactCardProps) {
  const category = getCategory(artifact.type);
  const showVersion = artifact.metadata.version > 1;

  const handleClick = () => {
    if (!disabled) onClick(artifact.id);
  };

  return (
    <button
      data-testid="artifact-card"
      data-selected={isSelected ? "true" : "false"}
      type="button"
      onClick={handleClick}
      aria-pressed={isSelected}
      aria-label={artifact.name}
      className="w-full p-3 rounded border text-left transition-colors hover:bg-[--bg-hover] focus:outline-none focus:ring-2 focus:ring-[--border-focus] disabled:opacity-50 disabled:cursor-not-allowed"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: isSelected ? "var(--accent-primary)" : "var(--border-subtle)",
      }}
      disabled={disabled}
    >
      <div className="flex items-start justify-between gap-2">
        <span data-testid="artifact-name" className="text-sm font-medium truncate" style={{ color: "var(--text-primary)" }}>
          {artifact.name}
        </span>
        <span
          data-testid="artifact-type-badge"
          data-category={category}
          className="px-1.5 py-0.5 text-xs rounded flex-shrink-0"
          style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-secondary)" }}
        >
          {TYPE_LABELS[artifact.type]}
        </span>
      </div>
      <div className="flex items-center gap-2 mt-2">
        <span data-testid="artifact-timestamp" className="text-xs" style={{ color: "var(--text-muted)" }}>
          {formatTimestamp(artifact.metadata.createdAt)}
        </span>
        {showVersion && (
          <span data-testid="artifact-version" className="text-xs px-1 rounded" style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-secondary)" }}>
            v{artifact.metadata.version}
          </span>
        )}
        {artifact.content.type === "inline" ? (
          <span data-testid="content-type-inline" className="text-xs" style={{ color: "var(--text-muted)" }} title="Inline content">
            📝
          </span>
        ) : (
          <span data-testid="content-type-file" className="text-xs" style={{ color: "var(--text-muted)" }} title="File content">
            📁
          </span>
        )}
      </div>
    </button>
  );
}
