import { Download, Eye } from "lucide-react";

import type { ProjectSkillExportResult } from "@/api/project-skills";
import { Button } from "@/components/ui/button";

interface ProjectSkillsExportControlsProps {
  disabled: boolean;
  onPreview: () => void;
  onApply: () => void;
}

export function ProjectSkillsExportControls({
  disabled,
  onPreview,
  onApply,
}: ProjectSkillsExportControlsProps) {
  return (
    <>
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
        disabled={disabled}
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
