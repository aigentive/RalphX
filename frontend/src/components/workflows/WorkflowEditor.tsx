/**
 * WorkflowEditor - Form for creating/editing workflow schemas
 *
 * Features:
 * - Name and description fields
 * - Column list with add/remove
 * - Column name and mapsTo status configuration
 * - Save and cancel actions
 */

import { useState, useCallback } from "react";
import type { WorkflowSchema, WorkflowColumn } from "@/types/workflow";
import { INTERNAL_STATUS_VALUES, type InternalStatus } from "@/types/status";

// ============================================================================
// Types
// ============================================================================

interface WorkflowEditorProps {
  workflow?: WorkflowSchema;
  onSave: (workflow: Omit<WorkflowSchema, "id"> & { id?: string }) => void;
  onCancel: () => void;
  isSaving?: boolean;
}

interface ColumnState {
  id: string;
  name: string;
  mapsTo: InternalStatus;
}

// ============================================================================
// Helpers
// ============================================================================

const createDefaultColumn = (): ColumnState => ({
  id: `col-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
  name: "",
  mapsTo: "backlog",
});

const STATUS_LABELS: Record<InternalStatus, string> = {
  backlog: "Backlog",
  ready: "Ready",
  blocked: "Blocked",
  executing: "Executing",
  qa_refining: "QA Refining",
  qa_testing: "QA Testing",
  qa_passed: "QA Passed",
  qa_failed: "QA Failed",
  pending_review: "Pending Review",
  revision_needed: "Revision Needed",
  approved: "Approved",
  failed: "Failed",
  cancelled: "Cancelled",
  reviewing: "AI Review in Progress",
  review_passed: "AI Review Passed",
  escalated: "Escalated",
  re_executing: "Re-executing",
  pending_merge: "Pending Merge",
  merging: "Merging",
  waiting_on_pr: "Waiting on PR",
  merge_incomplete: "Merge Incomplete",
  merge_conflict: "Merge Conflict",
  updating_plan_branch: "Updating Plan Branch",
  updating_task_branch: "Updating Task Branch",
  branch_update_blocked: "Branch Update Blocked",
  merged: "Merged",
  paused: "Paused",
  stopped: "Stopped",
};

// ============================================================================
// Component
// ============================================================================

export function WorkflowEditor({ workflow, onSave, onCancel, isSaving = false }: WorkflowEditorProps) {
  const [name, setName] = useState(workflow?.name ?? "");
  const [description, setDescription] = useState(workflow?.description ?? "");
  const [columns, setColumns] = useState<ColumnState[]>(
    workflow?.columns.map((c) => ({ id: c.id, name: c.name, mapsTo: c.mapsTo })) ?? [createDefaultColumn()]
  );

  const handleAddColumn = useCallback(() => {
    setColumns((prev) => [...prev, createDefaultColumn()]);
  }, []);

  const handleRemoveColumn = useCallback((id: string) => {
    setColumns((prev) => prev.filter((c) => c.id !== id));
  }, []);

  const handleColumnChange = useCallback((id: string, field: keyof ColumnState, value: string) => {
    setColumns((prev) =>
      prev.map((c) => (c.id === id ? { ...c, [field]: value } : c))
    );
  }, []);

  const handleSave = useCallback(() => {
    const workflowColumns: WorkflowColumn[] = columns.map((c) => ({
      id: c.id,
      name: c.name,
      mapsTo: c.mapsTo,
    }));
    const workflowData = {
      name,
      description,
      columns: workflowColumns,
      isDefault: workflow?.isDefault ?? false,
      ...(workflow?.id !== undefined && { id: workflow.id }),
    };
    onSave(workflowData);
  }, [workflow, name, description, columns, onSave]);

  return (
    <div data-testid="workflow-editor" className="p-4 rounded space-y-4" style={{ backgroundColor: "var(--bg-surface)" }}>
      <div className="space-y-2">
        <label htmlFor="workflow-name" className="block text-sm font-medium" style={{ color: "var(--text-primary)" }}>
          Workflow Name
        </label>
        <input id="workflow-name" data-testid="workflow-name-input" type="text" value={name} onChange={(e) => setName(e.target.value)}
          className="w-full px-3 py-2 rounded border text-sm" style={{ backgroundColor: "var(--bg-base)", borderColor: "var(--border-default)", color: "var(--text-primary)" }} />
      </div>
      <div className="space-y-2">
        <label htmlFor="workflow-description" className="block text-sm" style={{ color: "var(--text-secondary)" }}>Description</label>
        <input id="workflow-description" data-testid="workflow-description-input" type="text" value={description} onChange={(e) => setDescription(e.target.value)}
          className="w-full px-3 py-2 rounded border text-sm" style={{ backgroundColor: "var(--bg-base)", borderColor: "var(--border-default)", color: "var(--text-primary)" }} />
      </div>
      <div className="space-y-2">
        <div className="flex justify-between items-center">
          <span className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>Columns</span>
          <button data-testid="add-column-button" onClick={handleAddColumn} className="px-2 py-1 text-xs rounded" style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-primary)" }}>+ Add Column</button>
        </div>
        <div className="space-y-2">
          {columns.map((col, idx) => (
            <div key={col.id} data-testid="column-item" className="flex items-center gap-2 p-2 rounded" style={{ backgroundColor: "var(--bg-base)" }}>
              <input aria-label={`Column ${idx + 1} name`} data-testid="column-name-input" type="text" value={col.name} onChange={(e) => handleColumnChange(col.id, "name", e.target.value)}
                placeholder="Column name" className="flex-1 px-2 py-1 rounded border text-sm" style={{ borderColor: "var(--border-subtle)", color: "var(--text-primary)" }} />
              <select aria-label={`Column ${idx + 1} status`} data-testid="column-status-select" value={col.mapsTo} onChange={(e) => handleColumnChange(col.id, "mapsTo", e.target.value)}
                className="px-2 py-1 rounded border text-sm" style={{ borderColor: "var(--border-subtle)", color: "var(--text-primary)" }}>
                {INTERNAL_STATUS_VALUES.map((status) => (<option key={status} value={status}>{STATUS_LABELS[status]}</option>))}
              </select>
              {columns.length > 1 && (<button data-testid="remove-column-button" onClick={() => handleRemoveColumn(col.id)} className="p-1 rounded hover:bg-[--bg-hover]" style={{ color: "var(--text-muted)" }} aria-label="Remove column">×</button>)}
            </div>
          ))}
        </div>
      </div>
      <div className="flex justify-end gap-2 pt-2">
        <button data-testid="cancel-button" onClick={onCancel} disabled={isSaving} className="px-4 py-2 rounded text-sm disabled:opacity-50" style={{ backgroundColor: "var(--bg-hover)", color: "var(--text-primary)" }}>Cancel</button>
        <button data-testid="save-button" onClick={handleSave} disabled={isSaving} className="px-4 py-2 rounded text-sm font-medium disabled:opacity-50" style={{ backgroundColor: "var(--accent-primary)", color: "var(--text-on-accent)" }}>{isSaving ? "Saving..." : "Save"}</button>
      </div>
    </div>
  );
}
