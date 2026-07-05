import { Loader2 } from "lucide-react";

import type { AgentComposerPlanReference } from "@/api/agent-composer";
import { planTitle, previewText } from "./AgentPlanStartPanel.utils";

const elevatedSurfaceStyle = {
  backgroundColor: "var(--bg-elevated)",
  borderColor: "var(--overlay-faint)",
  borderWidth: 1,
  borderStyle: "solid",
} as const;

export function AgentPlanStartPanelPreview({
  selectedPlan,
  selectedVersion,
  versionOptions,
  isLoading,
  isError,
  preview,
  onVersionChange,
}: {
  selectedPlan: AgentComposerPlanReference | null;
  selectedVersion: number | null;
  versionOptions: Array<{ version: number; createdAt: string | null }>;
  isLoading: boolean;
  isError: boolean;
  preview: string | null;
  onVersionChange: (version: number) => void;
}) {
  if (!selectedPlan) {
    return (
      <div
        className="rounded-md px-3 py-4 text-sm"
        style={{
          backgroundColor: "var(--bg-elevated)",
          color: "var(--text-secondary)",
          borderColor: "var(--overlay-faint)",
          borderWidth: 1,
          borderStyle: "solid",
        }}
      >
        Select a project plan to preview it.
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col rounded-md" style={elevatedSurfaceStyle}>
      <div
        className="flex flex-wrap items-center justify-between gap-3 px-3 py-2"
        style={{
          borderBottomColor: "var(--overlay-faint)",
          borderBottomWidth: 1,
          borderBottomStyle: "solid",
        }}
      >
        <div className="min-w-0">
          <div className="truncate text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            {planTitle(selectedPlan)}
          </div>
          <div className="text-xs" style={{ color: "var(--text-muted)" }}>
            {selectedPlan.status}
          </div>
        </div>
        <label className="flex items-center gap-2 text-xs" style={{ color: "var(--text-muted)" }}>
          Version
          <select
            aria-label="Preview plan version"
            value={selectedVersion ?? selectedPlan.artifactVersion}
            onChange={(event) => onVersionChange(Number(event.target.value))}
            className="h-8 rounded-md px-2 text-sm"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--overlay-faint)",
              borderWidth: 1,
              borderStyle: "solid",
              color: "var(--text-primary)",
            }}
          >
            {versionOptions.map(({ version }) => (
              <option key={version} value={version}>
                v{version}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-3">
        {isLoading ? (
          <div className="flex items-center gap-2 text-sm" style={{ color: "var(--text-secondary)" }}>
            <Loader2 aria-hidden="true" className="h-4 w-4 animate-spin" />
            Loading preview...
          </div>
        ) : isError ? (
          <div className="text-sm" role="alert" style={{ color: "var(--status-error)" }}>
            Preview unavailable.
          </div>
        ) : preview ? (
          <pre
            className="whitespace-pre-wrap break-words text-xs leading-5"
            style={{ color: "var(--text-primary)" }}
          >
            {previewText(preview)}
          </pre>
        ) : (
          <div className="text-sm" style={{ color: "var(--text-secondary)" }}>
            No inline preview available.
          </div>
        )}
      </div>
    </div>
  );
}
