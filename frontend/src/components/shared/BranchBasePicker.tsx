import { useMemo, useState } from "react";
import { Check, ChevronDown, GitBranch, Loader2, Search } from "lucide-react";

import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import type { BranchBaseOption } from "./branchBaseOptions";

interface BranchBasePickerProps {
  value: string;
  onValueChange: (value: string) => void;
  options: BranchBaseOption[];
  placeholder: string;
  disabled?: boolean;
  readOnly?: boolean;
  testId?: string;
  className?: string;
  align?: "start" | "center" | "end";
  isLoading?: boolean;
  prefixLabel?: string;
  ariaLabel?: string;
  onIntent?: () => void;
  onOpenChange?: (open: boolean) => void;
}

export function BranchBasePicker({
  value,
  onValueChange,
  options,
  placeholder,
  disabled = false,
  readOnly = false,
  testId,
  className,
  align = "end",
  isLoading = false,
  prefixLabel = "Start from",
  ariaLabel = prefixLabel,
  onIntent,
  onOpenChange,
}: BranchBasePickerProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const selectedOption = options.find((option) => option.key === value) ?? null;
  const filteredOptions = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    if (!query) {
      return options;
    }
    return options.filter(
      (option) =>
        option.label.toLowerCase().includes(query) ||
        option.detail?.toLowerCase().includes(query) ||
        option.selection.ref.toLowerCase().includes(query)
    );
  }, [options, searchQuery]);

  const handleOpenChange = (open: boolean) => {
    onOpenChange?.(open);
    setIsOpen(open);
    if (!open) {
      setSearchQuery("");
    }
  };

  const handleSelect = (option: BranchBaseOption) => {
    onValueChange(option.key);
    setIsOpen(false);
    setSearchQuery("");
  };

  const trigger = (
    <button
      type="button"
      className={cn(
        "flex min-w-0 max-w-[min(100%,430px)] items-center gap-2 rounded-full px-2 py-1 text-[0.75rem] transition-colors",
        !readOnly && "hover:bg-[var(--bg-hover)]",
        "disabled:cursor-not-allowed disabled:opacity-60",
        className
      )}
      style={{ color: "var(--text-secondary)" }}
      disabled={disabled || readOnly}
      data-testid={testId}
      data-theme-button-skip="true"
      aria-label={ariaLabel}
      onFocus={onIntent}
      onPointerEnter={onIntent}
    >
      <GitBranch className="h-3.5 w-3.5 shrink-0" />
      <span className="shrink-0 text-[0.625rem] font-medium uppercase tracking-[0.14em]">
        {prefixLabel}
      </span>
      <span
        className="min-w-0 truncate font-medium"
        style={{ color: selectedOption ? "var(--text-primary)" : "var(--text-secondary)" }}
      >
        {selectedOption?.label ?? placeholder}
      </span>
      {isLoading ? (
        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
      ) : !readOnly ? (
        <ChevronDown className="h-3.5 w-3.5 shrink-0" />
      ) : null}
    </button>
  );

  if (readOnly) {
    return trigger;
  }

  return (
    <Popover open={isOpen} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      <PopoverContent
        align={align}
        className="w-[min(420px,calc(100vw-2rem))] p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <div className="border-b border-[var(--border-subtle)] p-2">
          <div className="relative">
            <Search
              className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              placeholder="Search branches..."
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              className="h-8 border-[var(--border-subtle)] bg-[var(--bg-surface)] pl-8 pr-2 text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
              style={{ outline: "none", boxShadow: "none" }}
              autoFocus
            />
          </div>
        </div>
        <div className="max-h-72 overflow-y-auto overscroll-contain">
          <div className="p-1">
            {isLoading && (
              <div
                className="mb-1 flex items-center gap-2 rounded-md px-2 py-1.5 text-xs"
                style={{
                  color: "var(--text-secondary)",
                  background: "var(--bg-surface)",
                }}
              >
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                <span>Refreshing branches...</span>
              </div>
            )}
            {filteredOptions.length === 0 ? (
              <div
                className="flex items-center justify-center py-6 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                No branches found
              </div>
            ) : (
              <div className="space-y-0.5">
                {filteredOptions.map((option) => {
                  const isSelected = option.key === value;
                  return (
                    <button
                      key={`${option.source}:${option.key}`}
                      type="button"
                      className={cn(
                        "flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors",
                        isSelected
                          ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
                          : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
                      )}
                      onClick={() => handleSelect(option)}
                    >
                      <span className="mt-0.5 flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                        {isSelected && <Check className="h-3.5 w-3.5" />}
                      </span>
                      <span className="min-w-0">
                        <span className="block whitespace-normal break-words font-medium leading-snug">
                          {option.label}
                        </span>
                        {option.detail && option.detail !== option.label && (
                          <span
                            className="mt-0.5 block whitespace-normal break-all font-mono text-[0.625rem] leading-snug"
                            style={{ color: isSelected ? "currentColor" : "var(--text-muted)" }}
                          >
                            {option.detail}
                          </span>
                        )}
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
