import { useEffect, useId, useMemo, useState } from "react";
import { Check, ChevronDown, Search, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

import { ticketSelectClassName, ticketSelectStyle } from "./ticket-select-styles";

export interface TicketSearchableSelectOption {
  value: string;
  label: string;
  description?: string | null | undefined;
  disabled?: boolean | undefined;
  leadingColor?: string | undefined;
  testId?: string | undefined;
}

interface TicketSearchableSelectProps {
  ariaLabel: string;
  value: string;
  options: TicketSearchableSelectOption[];
  onValueChange: (value: string) => void;
  placeholder?: string | undefined;
  searchPlaceholder?: string | undefined;
  emptyLabel?: string | undefined;
  disabled?: boolean | undefined;
  clearable?: boolean | undefined;
  clearValue?: string | undefined;
  clearLabel?: string | undefined;
  size?: "sm" | "md" | undefined;
  className?: string | undefined;
  pageSize?: number | undefined;
  testId?: string | undefined;
}

function optionMatchesQuery(option: TicketSearchableSelectOption, query: string): boolean {
  if (!query) {
    return true;
  }
  const haystack = `${option.label} ${option.description ?? ""}`.toLowerCase();
  return haystack.includes(query);
}

export function TicketSearchableSelect({
  ariaLabel,
  value,
  options,
  onValueChange,
  placeholder = "Select",
  searchPlaceholder = "Search...",
  emptyLabel = "No options found",
  disabled = false,
  clearable = false,
  clearValue = "",
  clearLabel,
  size = "sm",
  className,
  pageSize = 20,
  testId,
}: TicketSearchableSelectProps) {
  const listboxId = useId();
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [visibleCount, setVisibleCount] = useState(pageSize);
  const query = searchQuery.trim().toLowerCase();
  const selectedOption = options.find((option) => option.value === value);
  const selectedLabel = selectedOption?.label ?? placeholder;
  const showClear = clearable && value !== clearValue && !disabled;
  const clearActionLabel = clearLabel ?? `Clear ${ariaLabel.toLowerCase()}`;
  const filteredOptions = useMemo(
    () => options.filter((option) => optionMatchesQuery(option, query)),
    [options, query],
  );
  const visibleOptions = filteredOptions.slice(0, visibleCount);
  const hasMoreOptions = filteredOptions.length > visibleOptions.length;

  useEffect(() => {
    setVisibleCount(pageSize);
  }, [pageSize, query, options.length]);

  const resetSearch = () => {
    setSearchQuery("");
    setVisibleCount(pageSize);
  };

  const handleOpenChange = (nextOpen: boolean) => {
    setOpen(nextOpen);
    if (!nextOpen) {
      resetSearch();
    }
  };

  const handleSelect = (option: TicketSearchableSelectOption) => {
    if (option.disabled) {
      return;
    }
    onValueChange(option.value);
    setOpen(false);
    resetSearch();
  };

  const handleClear = () => {
    onValueChange(clearValue);
    setOpen(false);
    resetSearch();
  };

  const revealNextPage = () => {
    if (hasMoreOptions) {
      setVisibleCount((count) => count + pageSize);
    }
  };

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <div className="relative min-w-0">
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            role="combobox"
            aria-label={ariaLabel}
            aria-controls={listboxId}
            aria-expanded={open}
            aria-disabled={disabled || undefined}
            disabled={disabled}
            className={cn(
              ticketSelectClassName(
                size,
                cn(
                  "relative w-full justify-start text-left",
                  showClear ? "pr-14" : "pr-8",
                ),
                { nativeCaret: false },
              ),
              className,
            )}
            style={ticketSelectStyle}
            data-testid={testId}
            onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault();
                setOpen(true);
              }
            }}
          >
            <span className="flex min-w-0 flex-1 items-center gap-2">
              {selectedOption?.leadingColor ? (
                <span
                  className="h-2.5 w-2.5 shrink-0 rounded-full"
                  aria-hidden="true"
                  style={{ backgroundColor: selectedOption.leadingColor }}
                />
              ) : null}
              <span className="truncate">{selectedLabel}</span>
            </span>
            <ChevronDown
              className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--text-muted)]"
              aria-hidden="true"
            />
          </Button>
        </PopoverTrigger>
        {showClear ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                aria-label={clearActionLabel}
                className="absolute right-7 top-1/2 z-10 grid h-5 w-5 -translate-y-1/2 place-items-center rounded-sm text-[var(--text-muted)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]"
                onMouseDown={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                }}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  handleClear();
                }}
              >
                <X className="h-3.5 w-3.5" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="top" className="text-xs">
              {clearActionLabel}
            </TooltipContent>
          </Tooltip>
        ) : null}
      </div>
      <PopoverContent
        align="start"
        sideOffset={6}
        className="w-[min(360px,calc(100vw-2rem))] p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <div
          className="p-2"
          style={{
            borderBottomColor: "var(--border-subtle)",
            borderBottomStyle: "solid",
            borderBottomWidth: "1px",
          }}
        >
          <div className="relative">
            <Search
              className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--text-muted)]"
              aria-hidden="true"
            />
            <Input
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder={searchPlaceholder}
              aria-label={`Search ${ariaLabel.toLowerCase()}`}
              className="h-8 pl-8 pr-2 text-xs"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                boxShadow: "none",
                color: "var(--text-primary)",
                outline: "none",
              }}
              autoFocus
            />
          </div>
        </div>
        <div
          className="max-h-72 overflow-y-auto overscroll-contain p-1"
          onScroll={(event) => {
            const target = event.currentTarget;
            const remaining = target.scrollHeight - target.scrollTop - target.clientHeight;
            if (remaining < 32) {
              revealNextPage();
            }
          }}
        >
          <div id={listboxId} role="listbox" aria-label={ariaLabel}>
            {visibleOptions.length === 0 ? (
              <div className="px-3 py-6 text-center text-xs text-[var(--text-muted)]">
                {emptyLabel}
              </div>
            ) : (
              <div className="space-y-0.5">
                {visibleOptions.map((option) => {
                  const selected = option.value === value;
                  return (
                    <button
                      key={option.value}
                      type="button"
                      role="option"
                      aria-selected={selected}
                      disabled={option.disabled}
                      className={cn(
                        "flex w-full min-w-0 items-start gap-2 rounded-md px-2 py-2 text-left text-xs transition-colors",
                        selected
                          ? "bg-[var(--accent-muted)] text-[var(--accent-primary)]"
                          : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]",
                        option.disabled && "cursor-not-allowed opacity-50 hover:bg-transparent",
                      )}
                      onClick={() => handleSelect(option)}
                      data-testid={option.testId}
                    >
                      <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center">
                        {selected ? <Check className="h-4 w-4" aria-hidden="true" /> : null}
                      </span>
                      {option.leadingColor ? (
                        <span
                          className="mt-[5px] h-2.5 w-2.5 shrink-0 rounded-full"
                          aria-hidden="true"
                          style={{ backgroundColor: option.leadingColor }}
                        />
                      ) : null}
                      <span className="min-w-0">
                        <span className="block truncate text-[0.8125rem] font-medium leading-snug">
                          {option.label}
                        </span>
                        {option.description ? (
                          <span className="mt-0.5 block truncate text-[0.6875rem] leading-snug text-[var(--text-muted)]">
                            {option.description}
                          </span>
                        ) : null}
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
            {hasMoreOptions ? (
              <button
                type="button"
                className="mt-1 flex w-full items-center justify-center rounded-md px-2 py-2 text-xs font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                onClick={revealNextPage}
              >
                Show {Math.min(pageSize, filteredOptions.length - visibleOptions.length)} more
              </button>
            ) : null}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
