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
    <>
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
    </>
  );
}

export function ProjectSkillsExportSummary({
  preview,
}: {
  preview: ProjectSkillExportResult;
}) {
  const pendingExportCount = preview.files.filter((file) => file.willWrite).length;

  return (
    <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2 text-xs text-[var(--text-secondary)]">
      {pendingExportCount} pending file
      {pendingExportCount === 1 ? "" : "s"} in {preview.targetRoot}
    </div>
  );
}
