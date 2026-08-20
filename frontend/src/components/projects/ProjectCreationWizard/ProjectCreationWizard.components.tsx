/**
 * ProjectCreationWizard sub-components
 */

import type { ReactNode } from "react";
import { AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";

// ============================================================================
// RadioOption Component
// ============================================================================

interface RadioOptionProps {
  value: string;
  selected: boolean;
  onSelect: (value: string) => void;
  label: string;
  description: string;
  /** Radio group name — required so callers control keyboard arrow-key nav grouping. */
  name: string;
  warning?: string;
  testId: string;
  /** Optional leading icon shown instead of the plain radio dot. */
  icon?: ReactNode;
  children?: ReactNode;
}

export function RadioOption({
  value,
  selected,
  onSelect,
  label,
  description,
  name,
  warning,
  testId,
  icon,
  children,
}: RadioOptionProps) {
  return (
    <label
      data-testid={testId}
      data-selected={selected ? "true" : "false"}
      className={cn(
        "flex gap-3 p-3 rounded-lg cursor-pointer transition-colors",
        "focus-within:ring-2 focus-within:ring-[var(--accent-primary)] focus-within:ring-offset-2 focus-within:ring-offset-[var(--bg-base)]",
        selected
          ? "bg-[var(--bg-elevated)] border-[var(--accent-primary)]"
          : "bg-transparent border-[var(--border-subtle)] hover:bg-[var(--bg-hover)]"
      )}
      style={{
        border: `1px solid ${selected ? "var(--accent-primary)" : "var(--border-subtle)"}`,
      }}
    >
      <input
        type="radio"
        name={name}
        value={value}
        checked={selected}
        onChange={() => onSelect(value)}
        className="sr-only peer"
      />
      {icon ? (
        <span className="mt-0.5 flex-shrink-0 peer-focus-visible:ring-2 peer-focus-visible:ring-[var(--accent-primary)] peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-[var(--bg-base)] rounded">
          {icon}
        </span>
      ) : (
        <span
          className="mt-0.5 w-4 h-4 rounded-full border-2 flex items-center justify-center flex-shrink-0 peer-focus-visible:ring-2 peer-focus-visible:ring-[var(--accent-primary)] peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-[var(--bg-base)]"
          style={{
            borderColor: selected ? "var(--accent-primary)" : "var(--border-subtle)",
          }}
        >
          {selected && (
            <span
              className="w-2 h-2 rounded-full"
              style={{ backgroundColor: "var(--accent-primary)" }}
            />
          )}
        </span>
      )}
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium text-[var(--text-primary)]">
          {label}
        </div>
        <div className="text-xs mt-0.5 text-[var(--text-muted)]">
          {description}
        </div>
        {warning && (
          <div className="flex items-center gap-1.5 text-xs mt-1.5 text-[var(--status-warning)]">
            <AlertTriangle className="h-3.5 w-3.5" />
            <span>{warning}</span>
          </div>
        )}
        {selected && children && <div className="mt-3 space-y-3">{children}</div>}
      </div>
    </label>
  );
}
