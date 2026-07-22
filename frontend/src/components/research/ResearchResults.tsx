/**
 * ResearchResults - Displays research process results and artifacts
 *
 * Features:
 * - Process name and completion status
 * - Research question display
 * - Artifact list with type badges
 * - Link to artifact browser
 * - Error display for failed processes
 */

import type { Artifact, ArtifactType } from "@/types/artifact";
import type { ResearchProcess, ResearchProcessStatus } from "@/types/research";

// ============================================================================
// Types
// ============================================================================

interface ResearchResultsProps {
  process: ResearchProcess;
  artifacts: Artifact[];
  onViewArtifact: (artifactId: string) => void;
  onViewInBrowser: (bucketId: string) => void;
}

// ============================================================================
// Helpers
// ============================================================================

const STATUS_LABELS: Record<ResearchProcessStatus, string> = {
  pending: "Pending", running: "Running", paused: "Paused", completed: "Completed", failed: "Failed",
};

const STATUS_COLORS: Record<ResearchProcessStatus, string> = {
  pending: "var(--text-muted)", running: "var(--status-info)", paused: "var(--status-warning)",
  completed: "var(--status-success)", failed: "var(--status-error)",
};

const TYPE_LABELS: Record<ArtifactType, string> = {
  prd: "PRD", research_document: "Research Document", design_doc: "Design Doc", specification: "Specification",
  code_change: "Code Change", diff: "Diff", test_result: "Test Result", task_spec: "Task Spec",
  review_feedback: "Review Feedback", pr_review: "PR Review", approval: "Approval", findings: "Findings", recommendations: "Recommendations",
  context: "Context", previous_work: "Previous Work", research_brief: "Research Brief",
  activity_log: "Activity Log", alert: "Alert", intervention: "Intervention",
  persona: "Persona",
};

// ============================================================================
// Component
// ============================================================================

export function ResearchResults({ process, artifacts, onViewArtifact, onViewInBrowser }: ResearchResultsProps) {
  const { status, errorMessage } = process.progress;
  const hasArtifacts = artifacts.length > 0;

  return (
    <div data-testid="research-results" className="p-3 rounded border space-y-3" style={{ backgroundColor: "var(--bg-surface)", borderColor: "var(--border-subtle)" }}>
      {/* Header */}
      <div className="flex items-center justify-between">
        <span data-testid="process-name" className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>{process.name}</span>
        <span data-testid="status-badge" className="text-xs px-1.5 py-0.5 rounded"
          style={{ color: STATUS_COLORS[status], backgroundColor: "var(--bg-base)" }}>{STATUS_LABELS[status]}</span>
      </div>

      {/* Question */}
      <div className="text-sm" style={{ color: "var(--text-secondary)" }}>
        <span style={{ color: "var(--text-muted)" }}>Q: </span>
        <span data-testid="research-question">{process.brief.question}</span>
      </div>

      {/* Error Message */}
      {status === "failed" && errorMessage && (
        <div data-testid="error-message" className="text-xs px-2 py-1 rounded" style={{ backgroundColor: "var(--bg-base)", color: "var(--status-error)" }}>
          {errorMessage}
        </div>
      )}

      {/* Artifacts */}
      {hasArtifacts ? (
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <span data-testid="artifact-count" className="text-xs" style={{ color: "var(--text-muted)" }}>{artifacts.length} artifacts</span>
            <button data-testid="view-in-browser-button" onClick={() => onViewInBrowser(process.output.targetBucket)}
              className="text-xs px-2 py-1 rounded" style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-primary)" }}>View in Browser</button>
          </div>
          <div className="space-y-1">
            {artifacts.map((artifact) => (
              <button key={artifact.id} data-testid="artifact-item" onClick={() => onViewArtifact(artifact.id)} aria-label={artifact.name}
                className="w-full flex items-center justify-between px-2 py-1.5 rounded text-left text-sm hover:bg-[--bg-hover]"
                style={{ backgroundColor: "var(--bg-base)" }}>
                <span style={{ color: "var(--text-primary)" }}>{artifact.name}</span>
                <span data-testid="artifact-type" className="text-xs px-1 rounded" style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-muted)" }}>
                  {TYPE_LABELS[artifact.type]}
                </span>
              </button>
            ))}
          </div>
        </div>
      ) : (
        <div className="text-sm text-center py-2" style={{ color: "var(--text-muted)" }}>No artifacts produced</div>
      )}
    </div>
  );
}
