/**
 * TeamArtifactChips - Horizontal row of clickable artifact pills
 *
 * Renders below PlanDisplay. Each chip shows type icon + name.
 * Active chip gets warm orange border + subtle orange bg tint.
 * macOS Tahoe glass-morphism, warm orange accent.
 */

import React from "react";
import { Microscope, BarChart3, FileText } from "lucide-react";
import { cn } from "@/lib/utils";
import type { TeamArtifactSummary } from "@/api/team";

// ============================================================================
// Types
// ============================================================================

export interface TeamArtifactChipsProps {
  artifacts: TeamArtifactSummary[];
  selectedArtifactId: string | null;
  onSelect: (artifactId: string) => void;
}

// ============================================================================
// Helpers
// ============================================================================

interface ArtifactTypeConfig {
  icon: React.ElementType;
  color: string;
}

const DEFAULT_CONFIG: ArtifactTypeConfig = { icon: FileText, color: "var(--text-muted)" };

const ARTIFACT_TYPE_CONFIG: Record<string, ArtifactTypeConfig> = {
  research: { icon: Microscope, color: "var(--accent-primary)" },
  analysis: { icon: BarChart3, color: "var(--status-info)" },
  summary: DEFAULT_CONFIG,
};

function getTypeConfig(artifactType: string): ArtifactTypeConfig {
  return ARTIFACT_TYPE_CONFIG[artifactType.toLowerCase()] ?? DEFAULT_CONFIG;
}

// ============================================================================
// Component
// ============================================================================

export const TeamArtifactChips = React.memo(function TeamArtifactChips({
  artifacts,
  selectedArtifactId,
  onSelect,
}: TeamArtifactChipsProps) {
  if (artifacts.length === 0) return null;

  return (
    <div className="mt-3">
      {/* Section header */}
      <span
        className="text-[0.6875rem] font-medium tracking-wide block mb-2"
        style={{ color: "var(--text-muted)" }}
      >
        Team Research &middot; {artifacts.length} artifact{artifacts.length !== 1 ? "s" : ""}
      </span>

      {/* Chips row */}
      <div className="flex flex-wrap gap-2">
        {artifacts.map((artifact) => {
          const isActive = artifact.id === selectedArtifactId;
          const config = getTypeConfig(artifact.artifact_type);
          const Icon = config.icon;

          return (
            <button
              key={artifact.id}
              data-testid={`artifact-chip-${artifact.id}`}
              onClick={() => onSelect(artifact.id)}
              className={cn(
                "flex items-center gap-1.5 h-7 px-2.5 rounded-full",
                "text-[0.75rem] font-medium",
                "transition-all duration-150 ease-out",
                "cursor-pointer select-none",
              )}
              style={{
                background: isActive
                  ? "var(--accent-muted)"
                  : "var(--overlay-faint)",
                border: isActive
                  ? "1px solid var(--accent-border)"
                  : "1px solid var(--overlay-faint)",
                color: isActive
                  ? "var(--text-primary)"
                  : "var(--text-secondary)",
              }}
              onMouseEnter={(e) => {
                if (!isActive) {
                  e.currentTarget.style.background = "var(--overlay-faint)";
                  e.currentTarget.style.borderColor = "var(--overlay-moderate)";
                }
              }}
              onMouseLeave={(e) => {
                if (!isActive) {
                  e.currentTarget.style.background = "var(--overlay-faint)";
                  e.currentTarget.style.borderColor = "var(--overlay-faint)";
                }
              }}
            >
              <Icon
                className="w-3.5 h-3.5 flex-shrink-0"
                style={{ color: isActive ? config.color : "var(--text-muted)" }}
              />
              <span className="truncate max-w-[140px]">
                {artifact.name}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
});
