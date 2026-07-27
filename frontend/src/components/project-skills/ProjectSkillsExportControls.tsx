import { Download, Eye } from "lucide-react";

import type { ProjectSkillExportResult } from "@/api/project-skills";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";

interface ProjectSkillsExportControlsProps {
  disabled: boolean;
  exportEnabled: boolean;
  onExportEnabledChange: (enabled: boolean) => void;
  onPreview: () => void;
  onApply: () => void;
}

export function ProjectSkillsExportControls({
  disabled,
  exportEnabled,
  onExportEnabledChange,
  onPreview,
  onApply,
}: ProjectSkillsExportControlsProps) {
  return (
    <div className="grid gap-1">
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
          <Switch
            checked={exportEnabled}
            onCheckedChange={onExportEnabledChange}
            disabled={disabled}
            aria-label="Enable project skill export"
          />
          Export enabled
        </label>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onPreview}
          disabled={disabled}
        >
          <Eye />
          Preview export
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={onApply}
          disabled={disabled || !exportEnabled}
        >
          <Download />
          Export
        </Button>
      </div>
      <p className="max-w-[42rem] text-xs text-[var(--text-muted)]">
        Export writes approved or pinned skills to the target repo's
        .claude/skills (Claude) and .agents/skills (Codex) directories so either
        provider can reuse them. Apply requires a clean git worktree on a named
        review branch, not main/master/trunk.
      </p>
    </div>
  );
}

export function ProjectSkillsExportSummary({
  preview,
}: {
  preview: ProjectSkillExportResult;
}) {
  const pendingExportCount = preview.files.filter((file) => file.willWrite).length;

  return (
    <div className="grid gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--text-secondary)]">
      <div>
        {pendingExportCount} pending file
        {pendingExportCount === 1 ? "" : "s"} in {preview.targetRoot}
      </div>
      {preview.files.length > 0 ? (
        <ul className="grid gap-1">
          {preview.files.slice(0, 5).map((file) => (
            <li
              key={file.projectSkillId}
              className="flex min-w-0 items-center justify-between gap-3"
            >
              <span className="truncate font-mono">{file.relativePath}</span>
              <span className="shrink-0 text-[var(--text-muted)]">
                {file.willWrite ? "will write" : "unchanged"}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <div>No approved or pinned skills are eligible for export yet.</div>
      )}
      {preview.files.length > 5 ? (
        <div className="text-[var(--text-muted)]">
          {preview.files.length - 5} more file
          {preview.files.length - 5 === 1 ? "" : "s"} hidden.
        </div>
      ) : null}
    </div>
  );
}
