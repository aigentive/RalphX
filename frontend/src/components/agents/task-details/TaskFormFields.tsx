/**
 * TaskFormFields - Shared form field components for task forms
 *
 * Extracted from TaskEditForm to be reused by TaskCreationForm.
 * Provides consistent Refined Studio styling across all task forms.
 *
 * Design spec: specs/design/refined-studio-patterns.md
 */

import { useId } from "react";
import { TASK_CATEGORIES } from "@/types/task";
import { AlertCircle } from "lucide-react";
import {
  inputBaseStyles,
  selectBaseStyles,
  textareaBaseStyles,
  labelStyles,
  buttonPrimaryStyles,
  buttonSecondaryStyles,
} from "./TaskFormFields.constants";

// ============================================================================
// Form Field Components
// ============================================================================

interface TaskFormFieldsProps {
  title: string;
  setTitle: (value: string) => void;
  category: string;
  setCategory: (value: string) => void;
  description: string;
  setDescription: (value: string) => void;
  priority: number;
  setPriority: (value: number) => void;
  disabled?: boolean;
  /** Optional validation error to display */
  validationError?: string | null;
}

/**
 * Reusable form fields for task creation and editing
 */
export function TaskFormFields({
  title,
  setTitle,
  category,
  setCategory,
  description,
  setDescription,
  priority,
  setPriority,
  disabled = false,
  validationError,
}: TaskFormFieldsProps) {
  const baseId = useId();

  return (
    <div className="space-y-5">
      {/* Title Field */}
      <div>
        <label htmlFor={`${baseId}-title`} className={labelStyles}>
          Title
        </label>
        <input
          type="text"
          id={`${baseId}-title`}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          disabled={disabled}
          placeholder="Enter task title"
          className={inputBaseStyles}
        />
      </div>

      {/* Category & Priority Row */}
      <div className="grid grid-cols-2 gap-4">
        {/* Category Field */}
        <div>
          <label htmlFor={`${baseId}-category`} className={labelStyles}>
            Category
          </label>
          <select
            id={`${baseId}-category`}
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            disabled={disabled}
            className={selectBaseStyles}
          >
            {TASK_CATEGORIES.map((cat) => (
              <option key={cat} value={cat} className="bg-[var(--bg-elevated)] text-[var(--text-primary)]">
                {cat.charAt(0).toUpperCase() + cat.slice(1)}
              </option>
            ))}
          </select>
        </div>

        {/* Priority Field */}
        <div>
          <label htmlFor={`${baseId}-priority`} className={labelStyles}>
            Priority
          </label>
          <select
            id={`${baseId}-priority`}
            value={priority}
            onChange={(e) => setPriority(Number(e.target.value))}
            disabled={disabled}
            className={selectBaseStyles}
          >
            <option value={1} className="bg-[var(--bg-elevated)] text-[var(--text-primary)]">P1 - Critical</option>
            <option value={2} className="bg-[var(--bg-elevated)] text-[var(--text-primary)]">P2 - High</option>
            <option value={3} className="bg-[var(--bg-elevated)] text-[var(--text-primary)]">P3 - Medium</option>
            <option value={4} className="bg-[var(--bg-elevated)] text-[var(--text-primary)]">P4 - Low</option>
          </select>
        </div>
      </div>

      {/* Description Field */}
      <div>
        <label htmlFor={`${baseId}-description`} className={labelStyles}>
          Description
        </label>
        <textarea
          id={`${baseId}-description`}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          disabled={disabled}
          rows={4}
          placeholder="Enter task description (optional)"
          className={textareaBaseStyles}
        />
      </div>

      {/* Error Display - Tahoe flat styling */}
      {validationError && (
        <div
          className="flex items-center gap-2.5 px-3.5 py-3 rounded-lg text-[0.8125rem]"
          style={{
            backgroundColor: "var(--status-error-muted)",
            border: "1px solid var(--status-error-border)",
          }}
        >
          <AlertCircle className="w-4 h-4 shrink-0" style={{ color: "var(--status-error)" }} />
          <span style={{ color: "var(--status-error)" }}>{validationError}</span>
        </div>
      )}
    </div>
  );
}

// ============================================================================
// Form Actions Component
// ============================================================================

interface TaskFormActionsProps {
  onCancel: () => void;
  onSubmit?: () => void;
  submitLabel: string;
  submitLoadingLabel: string;
  isSubmitting: boolean;
  isDisabled: boolean;
}

/**
 * Reusable form action buttons (Cancel / Submit)
 */
export function TaskFormActions({
  onCancel,
  submitLabel,
  submitLoadingLabel,
  isSubmitting,
  isDisabled,
}: TaskFormActionsProps) {
  return (
    <div
      className="flex justify-end gap-3 pt-4 mt-auto border-t"
      style={{ borderColor: "var(--overlay-weak)" }}
    >
      <button
        type="button"
        onClick={onCancel}
        disabled={isSubmitting}
        className={buttonSecondaryStyles}
      >
        Cancel
      </button>
      <button
        type="submit"
        disabled={isSubmitting || isDisabled}
        className={buttonPrimaryStyles}
      >
        {isSubmitting ? submitLoadingLabel : submitLabel}
      </button>
    </div>
  );
}
