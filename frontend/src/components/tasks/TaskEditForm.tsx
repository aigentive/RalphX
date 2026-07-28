/**
 * TaskEditForm - Form for editing existing tasks
 *
 * Uses shared TaskFormFields for consistent Refined Studio styling.
 *
 * Features:
 * - Edit title, category, description, and priority
 * - Pre-populated with existing task data
 * - Form validation with Zod schema
 * - onSave callback for parent to handle mutation
 * - Step management section
 *
 * Design spec: specs/design/refined-studio-patterns.md
 */

import { useAgentGate, useFieldGate } from "@/hooks/useAgentGate";
import { useState, useCallback, type FormEvent } from "react";
import { UpdateTaskSchema, type Task, type UpdateTask } from "@/types/task";
import { ACTIVE_STATUSES } from "@/types/status";
import { Loader2, Plus } from "lucide-react";
import { StepList } from "./StepList";
import { useStepMutations } from "@/hooks/useStepMutations";
import { TaskFormFields, TaskFormActions } from "./TaskFormFields";
import {
  inputBaseStyles,
  labelStyles,
} from "./TaskFormFields.constants";

// ============================================================================
// Types
// ============================================================================

export interface TaskEditFormProps {
  /** Task to edit */
  task: Task;
  /** Callback when save is triggered (parent handles mutation) */
  onSave: (data: UpdateTask) => void;
  /** Callback when form is cancelled */
  onCancel: () => void;
  /** Whether the save operation is in progress */
  isSaving: boolean;
}

// ============================================================================
// Component
// ============================================================================

export function TaskEditForm({
  task,
  onSave,
  onCancel,
  isSaving,
}: TaskEditFormProps) {
  // Form state - pre-populate with task data
  const [title, setTitle] = useState(task.title);
  const [category, setCategory] = useState(task.category);
  const [description, setDescription] = useState(task.description || "");
  const [priority, setPriority] = useState(task.priority);
  const [validationError, setValidationError] = useState<string | null>(null);

  // Step editor state
  const [newStepTitle, setNewStepTitle] = useState("");
  const [isAddingStep, setIsAddingStep] = useState(false);
  const { create: createStep } = useStepMutations(task.id);

  // Check if task is executing (steps are editable only when not executing)
  const isExecuting = ACTIVE_STATUSES.includes(task.internalStatus);

  /**
   * Title and description are AGENT-CONSUMED content — editing them re-aims work in
   * flight — so they need `ui:agent` (2.6-b). Category and priority are inert under
   * the viewer-with-brakes boundary (A6).
   *
   * Both halves share one `update_task` call, so the gate has to be argument-level,
   * not command-level: the gated fields are stripped from the diff here AND their
   * inputs are disabled, so neither the form nor a stale state value can smuggle a
   * title change into an otherwise-inert priority edit.
   */
  // `update_task` is `class: operate`; the manifest's `conditional_capabilities`
  // escalates only `title`/`description` to agent control, so the gate is per-field.
  const agentGate = useFieldGate("update_task", "title");
  const stepGate = useAgentGate("stepCreate");

  const handleSubmit = useCallback(
    (e: FormEvent) => {
      e.preventDefault();
      setValidationError(null);

      // Build update data (only include changed fields)
      const updateData: UpdateTask = {};

      if (!agentGate.gated && title.trim() !== task.title) {
        updateData.title = title.trim();
      }

      if (category !== task.category) {
        updateData.category = category;
      }

      const descValue = description.trim() || null;
      if (!agentGate.gated && descValue !== task.description) {
        updateData.description = descValue;
      }

      if (priority !== task.priority) {
        updateData.priority = priority;
      }

      // Validate with Zod schema
      const result = UpdateTaskSchema.safeParse(updateData);
      if (!result.success) {
        setValidationError(result.error.issues[0]?.message || "Validation failed");
        return;
      }

      // If no fields changed, just cancel
      if (Object.keys(updateData).length === 0) {
        onCancel();
        return;
      }

      onSave(updateData);
    },
    [agentGate.gated, title, category, description, priority, task, onSave, onCancel]
  );

  const hasChanges = agentGate.gated
    ? category !== task.category || priority !== task.priority
    : title.trim() !== task.title ||
      category !== task.category ||
      (description.trim() || null) !== task.description ||
      priority !== task.priority;

  const handleAddStep = useCallback(async () => {
    if (!newStepTitle.trim()) return;

    setIsAddingStep(true);
    try {
      await createStep.mutateAsync({
        title: newStepTitle.trim(),
      });
      setNewStepTitle("");
    } finally {
      setIsAddingStep(false);
    }
  }, [newStepTitle, createStep]);

  return (
    <form onSubmit={handleSubmit} className="flex flex-col flex-1">
      {/* Form Fields */}
      <TaskFormFields
        title={title}
        setTitle={setTitle}
        category={category}
        setCategory={setCategory}
        description={description}
        setDescription={setDescription}
        priority={priority}
        setPriority={setPriority}
        disabled={isSaving}
        contentDisabled={agentGate.gated}
        contentDisabledReason={agentGate.reason}
        validationError={validationError}
      />

      {/* Steps Section */}
      <div
        className="rounded-lg p-4 mt-5"
        style={{
          background: "var(--overlay-faint)",
          border: "1px solid var(--overlay-weak)",
        }}
      >
        <div className="flex items-center justify-between mb-3">
          <label className={labelStyles + " mb-0"}>
            Steps
          </label>
          {isExecuting && (
            <span className="text-[0.6875rem] text-text-primary/40 italic">
              Cannot edit while executing
            </span>
          )}
        </div>

        {/* Step List */}
        <div className="mb-3">
          <StepList taskId={task.id} editable={!isExecuting && !isSaving} />
        </div>

        {/* Add Step Input */}
        {!isExecuting && (
          <div className="flex gap-2">
            <input
              type="text"
              value={newStepTitle}
              onChange={(e) => setNewStepTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  handleAddStep();
                }
              }}
              disabled={isSaving || isAddingStep}
              placeholder="Add a new step..."
              className={inputBaseStyles}
            />
            <button
              type="button"
              onClick={handleAddStep}
              disabled={
                isSaving || isAddingStep || stepGate.gated || !newStepTitle.trim()
              }
              className="h-10 px-3 rounded-lg text-[0.8125rem] font-medium shrink-0 flex items-center justify-center gap-1.5 transition-colors duration-150 disabled:opacity-40 disabled:cursor-not-allowed"
              style={{
                background: "transparent",
                border: "1px solid var(--overlay-moderate)",
                color: "var(--text-secondary)",
              }}
              onMouseEnter={(e) => {
                if (!isSaving && !isAddingStep && newStepTitle.trim()) {
                  e.currentTarget.style.borderColor = "color-mix(in srgb, var(--text-primary) 20%, transparent)";
                  e.currentTarget.style.color = "var(--text-secondary)";
                  e.currentTarget.style.background = "var(--overlay-faint)";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.borderColor = "var(--overlay-moderate)";
                e.currentTarget.style.color = "var(--text-secondary)";
                e.currentTarget.style.background = "transparent";
              }}
            >
              {isAddingStep ? (
                <Loader2 className="w-4 h-4 animate-spin" />
              ) : (
                <Plus className="w-4 h-4" />
              )}
              <span className="hidden sm:inline">{isAddingStep ? "Adding..." : "Add"}</span>
            </button>
          </div>
        )}
      </div>

      {/* Form Actions */}
      <TaskFormActions
        onCancel={onCancel}
        submitLabel="Save Changes"
        submitLoadingLabel="Saving..."
        isSubmitting={isSaving}
        isDisabled={!title.trim() || !hasChanges}
      />
    </form>
  );
}
