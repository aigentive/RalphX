import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { FileJson, FileText } from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";
import { useSessionExportImport } from "@/hooks/useSessionExportImport";
import type { Artifact } from "@/types/artifact";

// ============================================================================
// Types
// ============================================================================

export interface ExportPlanDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sessionId: string;
  sessionTitle: string | null;
  verificationStatus: string;
  planArtifact: Artifact | null;
  projectId: string;
}

// ============================================================================
// Helpers
// ============================================================================

function getVerificationBadgeLabel(status: string): string {
  if (status === "imported_verified") return "Verified (imported)";
  if (status === "verified") return "Verified";
  return status.replace(/_/g, " ");
}

// ============================================================================
// Component
// ============================================================================

export function ExportPlanDialog({
  open,
  onOpenChange,
  sessionId,
  sessionTitle,
  verificationStatus,
  planArtifact,
  projectId,
}: ExportPlanDialogProps) {
  const [isDownloadingMarkdown, setIsDownloadingMarkdown] = useState(false);

  const { exportSession, isExporting } = useSessionExportImport();

  const badgeLabel = getVerificationBadgeLabel(verificationStatus);

  const planContent =
    planArtifact?.content.type === "inline" ? planArtifact.content.text : "";

  const hasPlan = planArtifact !== null;
  const hasInlineContent = planArtifact !== null && planArtifact.content.type === "inline";

  const handleDownloadJson = async () => {
    await exportSession(sessionId, projectId, hasPlan);
  };

  const handleDownloadMarkdown = async () => {
    if (!planContent) return;

    setIsDownloadingMarkdown(true);
    try {
      const savePath = await save({
        filters: [{ name: "Markdown", extensions: ["md"] }],
        defaultPath: `${sessionTitle ?? "plan"}.md`,
      });

      if (savePath === null) {
        return;
      }

      await writeTextFile(savePath, planContent);
      toast.success("Plan exported as Markdown");
    } catch {
      toast.error("Failed to export plan as Markdown");
    } finally {
      setIsDownloadingMarkdown(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-w-md"
        aria-describedby={undefined}
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <DialogHeader>
          <DialogTitle style={{ color: "var(--text-primary)" }}>
            Export Plan
          </DialogTitle>
        </DialogHeader>

        {/* Source plan info */}
        <div
          className="rounded-lg px-3 py-2.5 text-sm"
          style={{
            backgroundColor: "var(--bg-surface)",
            border: "1px solid var(--border-subtle)",
          }}
        >
          <div className="flex items-center gap-2">
            <span
              className="font-medium truncate flex-1"
              style={{ color: "var(--text-primary)" }}
            >
              {sessionTitle ?? "Untitled plan"}
            </span>
            <span
              className="shrink-0 text-xs px-1.5 py-0.5 rounded"
              style={{
                backgroundColor:
                  "color-mix(in srgb, var(--accent-primary) 15%, transparent)",
                color: "var(--accent-primary)",
              }}
            >
              {badgeLabel}
            </span>
          </div>
        </div>

        {/* No plan message */}
        {!hasPlan && (
          <p className="text-sm" style={{ color: "var(--text-muted)" }}>
            No plan content available.
          </p>
        )}

        {/* Download cards */}
        <div className="flex flex-col gap-3">
          {/* JSON card */}
          <div
            className="rounded-lg p-3"
            style={{
              backgroundColor: "var(--bg-surface)",
              border: "1px solid var(--border-subtle)",
            }}
          >
            <div className="flex items-start gap-3">
              <FileJson
                className="w-5 h-5 mt-0.5 shrink-0"
                style={{ color: "var(--accent-primary)" }}
              />
              <div className="flex-1 min-w-0">
                <p
                  className="font-medium text-sm"
                  style={{ color: "var(--text-primary)" }}
                >
                  Download JSON
                </p>
                <p
                  className="text-xs mt-0.5"
                  style={{ color: "var(--text-muted)" }}
                >
                  Full session export with plan, proposals, and dependencies.
                  Re-importable into any RalphX project.
                </p>
              </div>
            </div>
            <div className="flex justify-end mt-3">
              <button
                onClick={handleDownloadJson}
                disabled={!hasPlan || isExporting}
                className="text-sm px-3 py-1.5 rounded-md font-medium transition-opacity disabled:opacity-50"
                style={{
                  backgroundColor: "var(--accent-primary)",
                  color: "white",
                }}
              >
                {isExporting ? "Exporting..." : "Download"}
              </button>
            </div>
          </div>

          {/* Markdown card */}
          <div
            className="rounded-lg p-3"
            style={{
              backgroundColor: "var(--bg-surface)",
              border: "1px solid var(--border-subtle)",
            }}
          >
            <div className="flex items-start gap-3">
              <FileText
                className="w-5 h-5 mt-0.5 shrink-0"
                style={{ color: "var(--accent-primary)" }}
              />
              <div className="flex-1 min-w-0">
                <p
                  className="font-medium text-sm"
                  style={{ color: "var(--text-primary)" }}
                >
                  Download Markdown
                </p>
                <p
                  className="text-xs mt-0.5"
                  style={{ color: "var(--text-muted)" }}
                >
                  Plan content as readable .md file for sharing or reference.
                </p>
              </div>
            </div>
            <div className="flex justify-end mt-3">
              <button
                onClick={handleDownloadMarkdown}
                disabled={!hasInlineContent || isDownloadingMarkdown}
                className="text-sm px-3 py-1.5 rounded-md font-medium transition-opacity disabled:opacity-50"
                style={{
                  backgroundColor: "var(--accent-primary)",
                  color: "white",
                }}
              >
                {isDownloadingMarkdown ? "Exporting..." : "Download"}
              </button>
            </div>
          </div>
        </div>

        {/* TODO: Re-enable cross-project export in future */}
        {/*
        <div>
          <h3>Export to Project</h3>
          <p>Create a copy of this verified plan in another project.</p>
          ...cross-project export form...
        </div>
        */}
      </DialogContent>
    </Dialog>
  );
}
