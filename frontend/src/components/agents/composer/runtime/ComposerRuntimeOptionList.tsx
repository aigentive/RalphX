import { useCallback, useEffect, useState, type ComponentType } from "react";

import { Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

import type { ComposerRuntimeOption } from "./runtimeSelectorTypes";

export function ComposerRuntimeOptionList({
  label,
  value,
  options,
  disabled,
  testId,
  icon: Icon,
  onValueChange,
  allowCustomValue = false,
  customPlaceholder = "Custom value",
}: {
  label: string;
  value: string;
  options: ComposerRuntimeOption[];
  disabled: boolean;
  testId?: string;
  icon: ComponentType<{ className?: string }>;
  onValueChange: (value: string) => void;
  allowCustomValue?: boolean;
  customPlaceholder?: string | undefined;
}) {
  const [customValue, setCustomValue] = useState("");
  const hasCurrentOption = options.some((option) => option.id === value);

  useEffect(() => {
    if (!hasCurrentOption) {
      setCustomValue(value);
    }
  }, [hasCurrentOption, value]);

  const commitCustomValue = useCallback(() => {
    const nextValue = customValue.trim();
    if (!nextValue || disabled) {
      return;
    }
    onValueChange(nextValue);
  }, [customValue, disabled, onValueChange]);

  return (
    <div className="py-1">
      <div className="flex items-center gap-1.5 px-2 py-1">
        <Icon className="h-3 w-3 text-[var(--text-muted)]" />
        <span className="text-[0.625rem] font-medium uppercase tracking-[0.14em] text-[var(--text-muted)]">
          {label}
        </span>
      </div>
      <div className="space-y-0.5">
        {options.map((option) => {
          const isSelected = option.id === value;
          const optionDisabled = disabled || option.disabled;
          return (
            <button
              key={option.id}
              type="button"
              disabled={optionDisabled}
              data-testid={testId ? `${testId}-${option.id}` : undefined}
              className={cn(
                "flex w-full items-start justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[0.75rem] transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                isSelected
                  ? "bg-[var(--accent-muted)]"
                  : "hover:bg-[var(--bg-hover)]",
              )}
              onClick={() => {
                if (!optionDisabled) {
                  onValueChange(option.id);
                }
              }}
            >
              <span className="min-w-0 flex-1">
                <span
                  className="block truncate"
                  style={{
                    color: isSelected
                      ? "var(--accent-primary)"
                      : "var(--text-primary)",
                    fontWeight: isSelected ? 600 : 500,
                  }}
                >
                  {option.label}
                </span>
                {(option.disabledReason || option.description) && (
                  <span
                    className="mt-0.5 block line-clamp-2 text-[0.6875rem] leading-snug"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {option.disabledReason ?? option.description}
                  </span>
                )}
              </span>
              {isSelected && (
                <Check className="mt-0.5 h-3.5 w-3.5 shrink-0 text-[var(--accent-primary)]" />
              )}
            </button>
          );
        })}
      </div>
      {allowCustomValue && (
        <div className="mt-1.5 flex items-center gap-1.5 px-1">
          <Input
            value={customValue}
            onChange={(event) => setCustomValue(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitCustomValue();
              }
            }}
            disabled={disabled}
            placeholder={customPlaceholder}
            data-testid={testId ? `${testId}-custom-input` : undefined}
            className="h-8 min-w-0 flex-1 rounded-md border-[var(--border-default)] bg-[var(--bg-surface)] px-2 text-[12px] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
          />
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={disabled || customValue.trim().length === 0}
            onClick={commitCustomValue}
            data-testid={testId ? `${testId}-custom-apply` : undefined}
            className="h-8 rounded-md px-2 text-[12px]"
          >
            Use
          </Button>
        </div>
      )}
    </div>
  );
}
