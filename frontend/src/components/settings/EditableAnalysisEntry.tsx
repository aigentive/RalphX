/**
 * EditableAnalysisEntry - Editable analysis entry card component
 *
 * Features:
 * - Expandable/collapsible entry card
 * - Inline text input for path, label, install
 * - Array field management for validate[] and worktree_setup[]
 * - Reset links for customized fields
 * - Remove button for user-added entries
 * - Visual indicator (accent border) for customized fields
 */

import { useState } from "react";
import { ChevronDown, ChevronRight, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { AnalysisEntry } from "./useAnalysisEditor";

interface EditableAnalysisEntryProps {
  entry: AnalysisEntry;
  entryIdx: number;
  onUpdateField<K extends keyof AnalysisEntry>(field: K, value: AnalysisEntry[K]): void;
  onResetField(field: keyof AnalysisEntry): void;
  onResetEntry(): void;
  onAddArrayItem(field: "validate" | "worktree_setup"): void;
  onRemoveArrayItem(field: "validate" | "worktree_setup", itemIdx: number): void;
  onUpdateArrayItem(field: "validate" | "worktree_setup", itemIdx: number, value: string): void;
  isFieldCustomized(field: keyof AnalysisEntry): boolean;
  isUserAdded: boolean;
}

/**
 * Editable text field with reset link
 */
function EditableField({
  label,
  value,
  onChange,
  onReset,
  placeholder,
  showReset,
  disabled,
}: {
  label: string;
  value: string | null;
  onChange: (value: string | null) => void;
  onReset: () => void;
  placeholder?: string;
  showReset: boolean;
  disabled?: boolean;
}) {
  const displayValue = value ?? "";
  const hasValue = displayValue.length > 0;

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between">
        <label className="text-[0.625rem] uppercase tracking-wider text-[var(--text-muted)] font-medium">
          {label}
        </label>
        {showReset && (
          <button
            type="button"
            onClick={onReset}
            className="text-xs text-[var(--accent-primary)] hover:underline"
          >
            Reset
          </button>
        )}
      </div>
      <div className="flex items-center gap-1.5">
        <Input
          value={displayValue}
          onChange={(e) => onChange(e.target.value || (label === "Install" ? null : ""))}
          placeholder={placeholder}
          disabled={disabled}
          className="flex-1 bg-[var(--bg-surface)] border-[var(--border-default)] focus:border-[var(--accent-primary)] focus:ring-[var(--accent-primary)] text-sm outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
          style={{ boxShadow: "none", outline: "none" }}
        />
        {label === "Install" && hasValue && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => onChange(null)}
            disabled={disabled}
            className="h-8 px-2 text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
            title="Clear"
          >
            <X className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
}

/**
 * Array field (validate[], worktree_setup[])
 */
function ArrayField({
  label,
  items,
  onAddItem,
  onRemoveItem,
  onUpdateItem,
  onReset,
  showReset,
  disabled,
}: {
  label: string;
  items: string[];
  onAddItem: () => void;
  onRemoveItem: (idx: number) => void;
  onUpdateItem: (idx: number, value: string) => void;
  onReset: () => void;
  showReset: boolean;
  disabled?: boolean;
}) {
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between">
        <label className="text-[0.625rem] uppercase tracking-wider text-[var(--text-muted)] font-medium">
          {label}
        </label>
        {showReset && (
          <button
            type="button"
            onClick={onReset}
            className="text-xs text-[var(--accent-primary)] hover:underline"
          >
            Reset
          </button>
        )}
      </div>
      <div className="space-y-1">
        {items.map((item, i) => (
          <div key={i} className="flex items-center gap-1.5">
            <Input
              value={item}
              onChange={(e) => onUpdateItem(i, e.target.value)}
              placeholder="Enter command..."
              disabled={disabled}
              className="flex-1 bg-[var(--bg-surface)] border-[var(--border-default)] focus:border-[var(--accent-primary)] focus:ring-[var(--accent-primary)] text-sm outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
              style={{ boxShadow: "none", outline: "none" }}
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => onRemoveItem(i)}
              disabled={disabled}
              className="h-8 px-2 text-xs text-[var(--text-muted)] hover:text-[var(--status-error)] hover:bg-[var(--accent-muted)]"
              title="Remove"
            >
              <Trash2 className="w-3.5 h-3.5" />
            </Button>
          </div>
        ))}
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={onAddItem}
        disabled={disabled}
        className="h-7 px-2 text-xs text-[var(--accent-primary)] hover:bg-[var(--accent-muted)]"
      >
        + Add {label === "Validate" ? "Command" : "Setup"}
      </Button>
    </div>
  );
}

/**
 * EditableAnalysisEntry component
 */
export function EditableAnalysisEntry({
  entry,
  entryIdx: _entryIdx,
  onUpdateField,
  onResetField,
  onResetEntry,
  onAddArrayItem,
  onRemoveArrayItem,
  onUpdateArrayItem,
  isFieldCustomized,
  isUserAdded,
}: EditableAnalysisEntryProps) {
  const [expanded, setExpanded] = useState(false);

  const pathCustomized = isFieldCustomized("path");
  const labelCustomized = isFieldCustomized("label");
  const installCustomized = isFieldCustomized("install");
  const validateCustomized = isFieldCustomized("validate");
  const setupCustomized = isFieldCustomized("worktree_setup");

  return (
    <div
      className="rounded-md border border-[var(--border-subtle)] overflow-hidden"
      style={{ background: "var(--overlay-faint)" }}
    >
      {/* Header */}
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-[var(--bg-hover)] transition-colors"
      >
        {expanded ? (
          <ChevronDown className="w-3.5 h-3.5 text-[var(--text-muted)] shrink-0" />
        ) : (
          <ChevronRight className="w-3.5 h-3.5 text-[var(--text-muted)] shrink-0" />
        )}
        <code className="text-xs text-[var(--accent-primary)] font-medium">{entry.path || "..."}</code>
        <span className="text-xs text-[var(--text-muted)]">{entry.label || "(Unnamed)"}</span>
        {(pathCustomized || labelCustomized || installCustomized || validateCustomized || setupCustomized) && (
          <div
            className="ml-auto w-2 h-2 rounded-full"
            style={{ backgroundColor: "var(--accent-primary)" }}
            title="This entry has customizations"
          />
        )}
      </button>

      {/* Expanded content */}
      {expanded && (
        <div className="px-3 pb-3 pt-2 space-y-3 border-t border-[var(--border-subtle)]">
          {/* Path field */}
          <div
            className={pathCustomized ? "border-l-2 border-[var(--accent-primary)] pl-2.5" : ""}
          >
            <EditableField
              label="Path"
              value={entry.path as string | null}
              onChange={(val) => onUpdateField("path", val || "")}
              onReset={() => onResetField("path")}
              placeholder="e.g., . or src-tauri/"
              showReset={pathCustomized}
            />
          </div>

          {/* Label field */}
          <div
            className={labelCustomized ? "border-l-2 border-[var(--accent-primary)] pl-2.5" : ""}
          >
            <EditableField
              label="Label"
              value={entry.label as string | null}
              onChange={(val) => onUpdateField("label", val || "")}
              onReset={() => onResetField("label")}
              placeholder="e.g., Frontend (React/TS)"
              showReset={labelCustomized}
            />
          </div>

          {/* Install field */}
          <div
            className={installCustomized ? "border-l-2 border-[var(--accent-primary)] pl-2.5" : ""}
          >
            <EditableField
              label="Install"
              value={entry.install}
              onChange={(val) => onUpdateField("install", val)}
              onReset={() => onResetField("install")}
              placeholder="e.g., npm install"
              showReset={installCustomized}
            />
          </div>

          {/* Validate field */}
          <div
            className={validateCustomized ? "border-l-2 border-[var(--accent-primary)] pl-2.5" : ""}
          >
            <ArrayField
              label="Validate"
              items={entry.validate}
              onAddItem={() => onAddArrayItem("validate")}
              onRemoveItem={(idx) => onRemoveArrayItem("validate", idx)}
              onUpdateItem={(idx, val) => onUpdateArrayItem("validate", idx, val)}
              onReset={() => onResetField("validate")}
              showReset={validateCustomized}
            />
          </div>

          {/* Worktree Setup field */}
          <div
            className={setupCustomized ? "border-l-2 border-[var(--accent-primary)] pl-2.5" : ""}
          >
            <ArrayField
              label="Worktree Setup"
              items={entry.worktree_setup}
              onAddItem={() => onAddArrayItem("worktree_setup")}
              onRemoveItem={(idx) => onRemoveArrayItem("worktree_setup", idx)}
              onUpdateItem={(idx, val) => onUpdateArrayItem("worktree_setup", idx, val)}
              onReset={() => onResetField("worktree_setup")}
              showReset={setupCustomized}
            />
          </div>

          {/* Footer: Reset button */}
          <div className="flex items-center gap-2 pt-2 border-t border-[var(--border-subtle)]">
            {!isUserAdded && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onResetEntry}
                className="h-7 px-2 text-xs text-[var(--accent-primary)] hover:bg-[var(--accent-muted)]"
              >
                Reset Entry
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
