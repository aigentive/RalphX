import { useId, useMemo, useState } from "react";
import { X } from "lucide-react";

import type { TicketingProvider } from "@/api/ticketing";
import type { TicketLabelOption } from "@/api/ticketing";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { TicketSearchableSelect } from "./TicketSearchableSelect";

export interface TicketLabelEditorProps {
  provider: TicketingProvider;
  labels: string[];
  /** Selectable team labels (Linear only); ignored for Jira free-text mode. */
  labelOptions?: TicketLabelOption[] | undefined;
  isLabelOptionsLoading?: boolean | undefined;
  isLabelPending?: boolean | undefined;
  /** Disabled reason; when set the editor is read-only and shows the reason. */
  disabledReason?: string | null | undefined;
  /** Called with the new FULL desired label set (set-semantics). */
  onSetLabels?: ((labels: string[]) => Promise<void> | void) | undefined;
}

const JIRA_SPACE_MESSAGE = "Jira labels can't contain spaces";

// WKWebView-safe chip paint/border longhands (Rule 6) with theme-token literals.
const CHIP_STYLE = {
  backgroundColor: "var(--bg-elevated)",
  borderColor: "var(--border-subtle)",
  borderStyle: "solid" as const,
  borderWidth: "1px",
};

function hasDuplicate(labels: string[], candidate: string): boolean {
  const needle = candidate.trim().toLowerCase();
  return labels.some((label) => label.trim().toLowerCase() === needle);
}

/**
 * Presentational add/remove editor for ticket labels.
 *
 * - **Jira**: free-text input; Enter/"Add" appends a new chip. Labels may not
 *   contain spaces (validated client-side).
 * - **Linear**: pick from the issue team's existing labels via a native select;
 *   free typing of brand-new labels is intentionally not offered (v1 scope).
 *
 * Every mutation computes the new full label set and calls `onSetLabels`
 * (set-semantics; last write wins). Removal chips use an icon-only button with
 * an accessible name plus the app tooltip per the icon-only-buttons rule.
 */
export function TicketLabelEditor({
  provider,
  labels,
  labelOptions,
  isLabelOptionsLoading,
  isLabelPending,
  disabledReason,
  onSetLabels,
}: TicketLabelEditorProps) {
  const [draft, setDraft] = useState("");
  const [validationError, setValidationError] = useState<string | null>(null);
  const inputId = useId();
  const disabled = Boolean(disabledReason) || Boolean(isLabelPending) || !onSetLabels;

  const availableOptions = useMemo(() => {
    const applied = new Set(labels.map((label) => label.trim().toLowerCase()));
    return (labelOptions ?? []).filter(
      (option) => !applied.has(option.name.trim().toLowerCase()),
    );
  }, [labelOptions, labels]);

  function commit(nextLabels: string[]) {
    void onSetLabels?.(nextLabels);
  }

  function handleRemove(label: string) {
    if (disabled) {
      return;
    }
    commit(labels.filter((existing) => existing !== label));
  }

  function handleAddJiraLabel() {
    const candidate = draft.trim();
    if (!candidate) {
      return;
    }
    if (/\s/.test(candidate)) {
      setValidationError(JIRA_SPACE_MESSAGE);
      return;
    }
    if (hasDuplicate(labels, candidate)) {
      setDraft("");
      setValidationError(null);
      return;
    }
    setValidationError(null);
    setDraft("");
    commit([...labels, candidate]);
  }

  function handleSelectLinearLabel(name: string) {
    if (!name || hasDuplicate(labels, name)) {
      return;
    }
    commit([...labels, name]);
  }

  if (disabledReason) {
    return (
      <div className="flex flex-col gap-1">
        <TicketChipRow labels={labels} onRemove={undefined} />
        <p className="text-[11px] text-[var(--text-muted)]">{disabledReason}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <TicketChipRow labels={labels} onRemove={handleRemove} disabled={disabled} />
      {provider === "linear" ? (
        <LinearLabelPicker
          inputId={inputId}
          options={availableOptions}
          isLoading={Boolean(isLabelOptionsLoading)}
          disabled={disabled}
          onSelect={handleSelectLinearLabel}
        />
      ) : (
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <input
              id={inputId}
              type="text"
              value={draft}
              disabled={disabled}
              placeholder="Add a label"
              aria-label="Add a label"
              className="h-8 flex-1 rounded-md px-2 text-sm outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px] disabled:cursor-not-allowed disabled:opacity-50"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                boxShadow: "none",
              }}
              onChange={(event) => {
                setDraft(event.target.value);
                if (validationError) {
                  setValidationError(null);
                }
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  handleAddJiraLabel();
                }
              }}
            />
            <button
              type="button"
              disabled={disabled || draft.trim().length === 0}
              className="h-8 shrink-0 rounded-md px-3 text-sm font-medium text-[var(--text-secondary)] outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px] disabled:cursor-not-allowed disabled:opacity-50"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
              onClick={handleAddJiraLabel}
            >
              Add
            </button>
          </div>
          {validationError && (
            <p role="alert" className="text-[11px] text-[var(--status-error)]">
              {validationError}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function TicketChipRow({
  labels,
  onRemove,
  disabled,
}: {
  labels: string[];
  onRemove?: ((label: string) => void) | undefined;
  disabled?: boolean | undefined;
}) {
  if (labels.length === 0) {
    return <p className="text-[11px] text-[var(--text-muted)]">No labels yet.</p>;
  }
  return (
    <span className="inline-flex min-w-0 flex-wrap items-center gap-1 text-[11px]">
      {labels.map((label) => (
        <span
          key={label}
          className="inline-flex max-w-[160px] items-center gap-1 rounded px-2 py-1"
          style={CHIP_STYLE}
        >
          <span className="truncate">{label}</span>
          {onRemove && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  disabled={disabled}
                  aria-label={`Remove label ${label}`}
                  className="inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded text-[var(--text-muted)] outline-none hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] disabled:cursor-not-allowed disabled:opacity-50"
                  onClick={() => onRemove(label)}
                >
                  <X className="h-3 w-3" aria-hidden="true" />
                </button>
              </TooltipTrigger>
              <TooltipContent>Remove label</TooltipContent>
            </Tooltip>
          )}
        </span>
      ))}
    </span>
  );
}

function LinearLabelPicker({
  inputId,
  options,
  isLoading,
  disabled,
  onSelect,
}: {
  inputId: string;
  options: TicketLabelOption[];
  isLoading: boolean;
  disabled: boolean;
  onSelect: (name: string) => void;
}) {
  if (isLoading) {
    return <p className="text-[11px] text-[var(--text-muted)]">Loading team labels…</p>;
  }
  if (options.length === 0) {
    return (
      <p className="text-[11px] text-[var(--text-muted)]">
        No more team labels available to add.
      </p>
    );
  }
  return (
    <div id={inputId}>
      <TicketSearchableSelect
        ariaLabel="Add a label"
        disabled={disabled}
        value=""
        className="w-full"
        placeholder="Add a label..."
        searchPlaceholder="Search labels..."
        emptyLabel="No labels found"
        onValueChange={(name) => {
          if (name) {
            onSelect(name);
          }
        }}
        options={options.map((option) => ({
          value: option.name,
          label: option.name,
        }))}
      />
    </div>
  );
}
