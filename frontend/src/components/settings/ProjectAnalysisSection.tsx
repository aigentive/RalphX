/**
 * ProjectAnalysisSection - Orchestrates inline-editable analysis settings
 *
 * Features:
 * - Editable analysis entries with per-field reset
 * - Refresh Detected Commands button to trigger project analyzer agent
 * - Batch save button (appears when isDirty)
 * - Template variables reference
 *
 * Delegates state management to useAnalysisEditor hook and
 * rendering to EditableAnalysisEntry component.
 */

import { useState, useCallback, useEffect } from "react";
import { Search, Loader2, ChevronDown, ChevronRight, Info, Plus } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";
import { useEventBus } from "@/providers/EventProvider";
import { SectionCard } from "./SettingsView.shared";
import { useAnalysisEditor } from "./useAnalysisEditor";
import { EditableAnalysisEntry } from "./EditableAnalysisEntry";

function formatTimestamp(iso: string | null): string {
  if (!iso) return "Never";
  try {
    const date = new Date(iso);
    return date.toLocaleString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "Unknown";
  }
}

/** Template variables reference panel */
function TemplateVariablesInfo() {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="mt-3">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1.5 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
      >
        <Info className="w-3 h-3" />
        <span>Template Variables</span>
        {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
      </button>
      {expanded && (
        <div
          className="mt-2 rounded-md px-3 py-2 space-y-1"
          style={{
            background: "var(--accent-muted)",
            border: "1px solid var(--accent-border)",
          }}
        >
          <div className="flex items-start gap-2">
            <code className="text-[0.6875rem] text-[var(--accent-primary)] whitespace-nowrap">{"{project_root}"}</code>
            <span className="text-[0.6875rem] text-[var(--text-muted)]">Project working directory</span>
          </div>
          <div className="flex items-start gap-2">
            <code className="text-[0.6875rem] text-[var(--accent-primary)] whitespace-nowrap">{"{worktree_path}"}</code>
            <span className="text-[0.6875rem] text-[var(--text-muted)]">Task worktree directory (when available)</span>
          </div>
          <div className="flex items-start gap-2">
            <code className="text-[0.6875rem] text-[var(--accent-primary)] whitespace-nowrap">{"{task_branch}"}</code>
            <span className="text-[0.6875rem] text-[var(--text-muted)]">Task branch name (when available)</span>
          </div>
        </div>
      )}
    </div>
  );
}

export function ProjectAnalysisSection() {
  const project = useProjectStore(selectActiveProject);
  const updateProject = useProjectStore((s) => s.updateProject);
  const bus = useEventBus();

  const [isAnalyzing, setIsAnalyzing] = useState(false);

  // Use the analysis editor hook for state management
  const editor = useAnalysisEditor(project, (customAnalysis) => {
    if (project) {
      updateProject(project.id, { customAnalysis });
    }
  });

  // Listen for analysis completion/failure events
  useEffect(() => {
    const unsubComplete = bus.subscribe<{
      project_id: string;
      detected_analysis: string | null;
      analyzed_at: string | null;
    }>("project:analysis_complete", (payload) => {
      if (project && payload.project_id === project.id) {
        updateProject(project.id, {
          detectedAnalysis: payload.detected_analysis,
          analyzedAt: payload.analyzed_at,
        });
        setIsAnalyzing(false);
        toast.success("Project analysis complete");
      }
    });

    const unsubFailed = bus.subscribe<{
      project_id: string;
      error: string;
    }>("project:analysis_failed", (payload) => {
      if (project && payload.project_id === project.id) {
        setIsAnalyzing(false);
        toast.error(`Analysis failed: ${payload.error}`);
      }
    });

    return () => {
      unsubComplete();
      unsubFailed();
    };
  }, [bus, project, updateProject]);

  const handleReanalyze = useCallback(async () => {
    if (!project) return;
    setIsAnalyzing(true);
    try {
      await api.projects.reanalyzeProject(project.id);
      toast.success("Re-analysis started. Results will appear shortly.");
      // isAnalyzing stays true — cleared by project:analysis_complete or project:analysis_failed event
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to start re-analysis");
      setIsAnalyzing(false);
    }
  }, [project]);

  if (!project) return null;

  return (
    <SectionCard
      icon={<Search className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />}
      title="Setup & Validation"
      description="Build system detection and validation commands"
    >
      {/* Header: Last Analyzed + Refresh Detected Commands */}
      <div className="flex items-center justify-between py-2">
        <span className="text-xs text-[var(--text-muted)]">
          Last Analyzed: {formatTimestamp(project.analyzedAt)}
        </span>
        <Button
          variant="ghost"
          size="sm"
          onClick={handleReanalyze}
          disabled={isAnalyzing}
          className="h-7 px-2.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
        >
          {isAnalyzing ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin mr-1" />
          ) : (
            <Search className="w-3.5 h-3.5 mr-1" />
          )}
          Refresh Detected Commands
        </Button>
      </div>

      {/* Editable Analysis Entries */}
      <div className="py-2 border-t border-[var(--border-subtle)]">
        {editor.entries.length > 0 ? (
          <div className="space-y-1.5">
            {editor.entries.map((entry, i) => (
              <EditableAnalysisEntry
                key={`${entry.path}-${i}`}
                entry={entry}
                entryIdx={i}
                onUpdateField={(field, value) => editor.updateField(i, field, value)}
                onResetField={(field) => editor.resetField(i, field)}
                onResetEntry={() => editor.resetEntry(i)}
                onAddArrayItem={(field) => editor.addArrayItem(i, field)}
                onRemoveArrayItem={(field, itemIdx) => editor.removeArrayItem(i, field, itemIdx)}
                onUpdateArrayItem={(field, itemIdx, value) => editor.updateArrayItem(i, field, itemIdx, value)}
                isFieldCustomized={(field) => editor.isFieldCustomized(i, field)}
                isUserAdded={editor.isUserAdded(i)}
              />
            ))}
          </div>
        ) : (
          <p className="text-xs text-[var(--text-muted)] italic py-2">
            {project.analyzedAt
              ? "No build systems detected"
              : "Not yet analyzed. Click Refresh Detected Commands to detect build systems."}
          </p>
        )}

        {/* Add Entry Button */}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => editor.addEntry()}
          className="mt-2 h-7 px-2 text-xs text-[var(--accent-primary)] hover:bg-[var(--accent-muted)]"
        >
          <Plus className="w-3.5 h-3.5 mr-1" />
          Add Entry
        </Button>
      </div>

      {/* Dirty Footer Bar (appears when isDirty) */}
      {editor.isDirty && (
        <div
          className="mt-3 px-3 py-2 rounded-md flex items-center justify-between"
          style={{
            background: "var(--accent-muted)",
            border: "1px solid var(--accent-border)",
          }}
        >
          <span className="text-xs text-[var(--text-muted)]">Analysis settings have unsaved changes</span>
          <div className="flex items-center gap-1.5">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => editor.resetAll()}
              disabled={editor.isSaving}
              className="h-7 px-2 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
            >
              Reset All
            </Button>
            <Button
              type="button"
              size="sm"
              onClick={() => editor.save()}
              disabled={editor.isSaving}
              className="h-7 px-2 text-xs bg-[var(--accent-primary)] text-[var(--text-on-accent)] hover:bg-[#ff5922]"
            >
              {editor.isSaving ? (
                <Loader2 className="w-3 h-3 animate-spin mr-1" />
              ) : null}
              Save
            </Button>
          </div>
        </div>
      )}

      {/* Template Variables Reference */}
      <TemplateVariablesInfo />
    </SectionCard>
  );
}
