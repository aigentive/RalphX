/**
 * MethodologyConfig - Displays active methodology configuration
 *
 * Features:
 * - Methodology name and description
 * - Workflow columns with color chips
 * - Phase progression diagram with arrows
 * - Agent profiles list with roles
 */

import type { MethodologyExtension } from "@/types/methodology";

// ============================================================================
// Types
// ============================================================================

interface MethodologyConfigProps {
  methodology: MethodologyExtension | null;
}

// ============================================================================
// Constants
// ============================================================================

const COLUMN_COLORS = ["#4ade80", "#facc15", "#60a5fa", "#a78bfa", "#f472b6", "#f97316"];

// ============================================================================
// Component
// ============================================================================

export function MethodologyConfig({ methodology }: MethodologyConfigProps) {
  if (!methodology) {
    return (
      <div data-testid="methodology-config" className="p-4 rounded" style={{ backgroundColor: "var(--bg-surface)" }}>
        <p className="text-sm text-center" style={{ color: "var(--text-muted)" }}>No active methodology</p>
      </div>
    );
  }

  return (
    <div data-testid="methodology-config" className="p-3 rounded space-y-4" style={{ backgroundColor: "var(--bg-surface)" }}>
      {/* Header */}
      <div>
        <h3 data-testid="methodology-name" className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>{methodology.name}</h3>
        <p data-testid="methodology-description" className="text-xs mt-0.5" style={{ color: "var(--text-secondary)" }}>{methodology.description}</p>
      </div>

      {/* Workflow */}
      <div data-testid="workflow-section">
        <h4 className="text-xs font-medium mb-2" style={{ color: "var(--text-muted)" }}>Workflow</h4>
        <div className="flex flex-wrap gap-2">
          {methodology.workflow.columns.map((col, idx) => (
            <div key={idx} data-testid="workflow-column" className="flex items-center gap-1.5 px-2 py-1 rounded text-xs"
              style={{ backgroundColor: "var(--bg-base)" }}>
              <span data-testid="column-chip" className="w-2 h-2 rounded-full" style={{ backgroundColor: COLUMN_COLORS[idx % COLUMN_COLORS.length] }} />
              <span style={{ color: "var(--text-primary)" }}>{col.name}</span>
              <span data-testid="mapped-status" className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>({col.mapsTo})</span>
            </div>
          ))}
        </div>
      </div>

      {/* Phases */}
      <div data-testid="phases-section">
        <h4 className="text-xs font-medium mb-2" style={{ color: "var(--text-muted)" }}>Phases</h4>
        <ul className="flex flex-wrap items-center gap-1">
          {methodology.phases.map((phase, idx) => (
            <li key={phase.id} className="flex items-center">
              <div data-testid="phase-item" className="flex items-center gap-1.5 px-2 py-1 rounded text-xs" style={{ backgroundColor: "var(--bg-base)" }}>
                <span data-testid="phase-order" className="w-4 h-4 flex items-center justify-center rounded-full text-[0.625rem]"
                  style={{ backgroundColor: "var(--accent-primary)", color: "var(--text-on-accent)" }}>{phase.order}</span>
                <span data-testid="phase-name" style={{ color: "var(--text-primary)" }}>{phase.name}</span>
              </div>
              {idx < methodology.phases.length - 1 && (
                <span data-testid="phase-arrow" className="mx-1 text-xs" style={{ color: "var(--text-muted)" }}>→</span>
              )}
            </li>
          ))}
        </ul>
      </div>

      {/* Agents */}
      <div data-testid="agents-section">
        <h4 className="text-xs font-medium mb-2" style={{ color: "var(--text-muted)" }}>Agents</h4>
        <ul className="flex flex-wrap gap-1">
          {methodology.agentProfiles.map((profileId) => (
            <li key={profileId} data-testid="agent-item" className="px-2 py-1 rounded text-xs"
              style={{ backgroundColor: "var(--bg-base)", color: "var(--text-primary)" }}>
              {profileId}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
