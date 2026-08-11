import { useEffect, useMemo, useState } from "react";
import {
  Check,
  ChevronDown,
  GitBranch,
  GitPullRequest,
  Info,
  Loader2,
  Search,
} from "lucide-react";

import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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
  enablePullRequests?: boolean;
  pullRequestOptions?: BranchBaseOption[];
  isLoadingPullRequests?: boolean;
  pullRequestMessage?: string | null;
  onPullRequestSearch?: (query: string) => void;
  closeOnSelect?: boolean;
  isolatedBranch?: boolean;
  onIsolatedBranchChange?: (checked: boolean) => void;
  isolatedBranchDisabled?: boolean;
}

type BranchBasePickerTab = "branches" | "pull_requests";

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
  enablePullRequests = false,
  pullRequestOptions = [],
  isLoadingPullRequests = false,
  pullRequestMessage = null,
  onPullRequestSearch,
  closeOnSelect = true,
  isolatedBranch,
  onIsolatedBranchChange,
  isolatedBranchDisabled = false,
}: BranchBasePickerProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<BranchBasePickerTab>("branches");
  const [branchSearchQuery, setBranchSearchQuery] = useState("");
  const [pullRequestSearchQuery, setPullRequestSearchQuery] = useState("");
  const showPullRequestTab = enablePullRequests || pullRequestOptions.length > 0;
  const allOptions = useMemo(
    () => (showPullRequestTab ? [...options, ...pullRequestOptions] : options),
    [options, pullRequestOptions, showPullRequestTab]
  );
  const selectedOption = allOptions.find((option) => option.key === value) ?? null;
  const filteredOptions = useMemo(() => {
    const query = branchSearchQuery.trim().toLowerCase();
    if (!query) {
      return options;
    }
    return options.filter(
      (option) =>
        option.label.toLowerCase().includes(query) ||
        option.detail?.toLowerCase().includes(query) ||
        option.selection.ref.toLowerCase().includes(query)
    );
  }, [options, branchSearchQuery]);

  useEffect(() => {
    if (!isOpen || !showPullRequestTab || activeTab !== "pull_requests" || !onPullRequestSearch) {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      onPullRequestSearch(pullRequestSearchQuery);
    }, 250);

    return () => window.clearTimeout(timeoutId);
  }, [
    activeTab,
    isOpen,
    onPullRequestSearch,
    pullRequestSearchQuery,
    showPullRequestTab,
  ]);

  const visibleOptions =
    activeTab === "pull_requests" ? pullRequestOptions : filteredOptions;
  const visibleLoading =
    activeTab === "pull_requests" ? isLoadingPullRequests : isLoading;
  const searchQuery =
    activeTab === "pull_requests" ? pullRequestSearchQuery : branchSearchQuery;
  const emptyLabel =
    activeTab === "pull_requests" ? "No pull requests found" : "No branches found";
  const loadingLabel =
    activeTab === "pull_requests" ? "Searching pull requests..." : "Refreshing branches...";
  const isolatedBranchTooltip =
    selectedOption?.selection.kind === "project_default"
      ? "Default-branch starts always create a new RalphX branch."
      : selectedOption?.selection.kind === "current_branch"
        ? "Current-branch starts always create a new RalphX branch because the selected branch is already checked out in the project root."
        : isolatedBranchDisabled
          ? "This start mode requires branch isolation, creating a separate RalphX branch and worktree from the selected base."
          : "Starts isolated by default, creating a separate RalphX branch and worktree from the selected base. Turn this off to work directly on the selected branch or PR.";

  const handleOpenChange = (open: boolean) => {
    onOpenChange?.(open);
    setIsOpen(open);
    if (!open) {
      setActiveTab("branches");
      setBranchSearchQuery("");
      setPullRequestSearchQuery("");
    }
  };

  const handleSelect = (option: BranchBaseOption) => {
    onValueChange(option.key);
    if (closeOnSelect) {
      setIsOpen(false);
      setActiveTab("branches");
      setBranchSearchQuery("");
      setPullRequestSearchQuery("");
    }
  };

  const handleSearchChange = (query: string) => {
    if (activeTab === "pull_requests") {
      setPullRequestSearchQuery(query);
    } else {
      setBranchSearchQuery(query);
    }
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
          {showPullRequestTab && (
            <div
              className="mb-2 grid grid-cols-2 rounded-md p-0.5"
              style={{ backgroundColor: "var(--bg-surface)" }}
              role="tablist"
              aria-label="Base reference type"
            >
              <button
                type="button"
                role="tab"
                aria-selected={activeTab === "branches"}
                className={cn(
                  "flex h-7 items-center justify-center gap-1.5 rounded px-2 text-xs font-medium transition-colors",
                  activeTab === "branches"
                    ? "bg-[var(--bg-elevated)] text-[var(--text-primary)]"
                    : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                )}
                onClick={() => setActiveTab("branches")}
              >
                <GitBranch className="h-3.5 w-3.5" />
                Branches
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={activeTab === "pull_requests"}
                className={cn(
                  "flex h-7 items-center justify-center gap-1.5 rounded px-2 text-xs font-medium transition-colors",
                  activeTab === "pull_requests"
                    ? "bg-[var(--bg-elevated)] text-[var(--text-primary)]"
                    : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                )}
                onClick={() => setActiveTab("pull_requests")}
              >
                <GitPullRequest className="h-3.5 w-3.5" />
                PRs
              </button>
            </div>
          )}
          <div className="relative">
            <Search
              className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              placeholder={
                activeTab === "pull_requests"
                  ? "Search pull requests..."
                  : "Search branches..."
              }
              value={searchQuery}
              onChange={(event) => handleSearchChange(event.target.value)}
              className="h-8 border-[var(--border-subtle)] bg-[var(--bg-surface)] pl-8 pr-2 text-xs text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
              style={{ outline: "none", boxShadow: "none" }}
              autoFocus
            />
          </div>
          {typeof isolatedBranch === "boolean" && onIsolatedBranchChange ? (
            <div
              className="mt-2 flex items-center justify-between gap-3 rounded-md border px-2.5 py-2"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderColor: "var(--border-subtle)",
              }}
            >
              <div className="flex min-w-0 items-center gap-1.5">
                <span className="min-w-0 text-xs font-medium text-[var(--text-primary)]">
                  Isolated branch
                </span>
                <TooltipProvider delayDuration={250}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        aria-label="About isolated branch"
                        className="inline-flex h-5 w-5 shrink-0 cursor-help items-center justify-center rounded-full border-0 bg-transparent p-0 text-[var(--text-muted)] transition-colors hover:text-[var(--text-secondary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]"
                      >
                        <Info className="h-3.5 w-3.5" aria-hidden="true" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent className="max-w-64 leading-snug">
                      {isolatedBranchTooltip}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
              <Switch
                checked={isolatedBranch}
                disabled={isolatedBranchDisabled}
                onCheckedChange={onIsolatedBranchChange}
                aria-label="Use isolated branch"
                className="data-[state=checked]:bg-[var(--accent-primary)] data-[state=unchecked]:bg-[var(--border-subtle)]"
              />
            </div>
          ) : null}
        </div>
        <div className="max-h-72 overflow-y-auto overscroll-contain">
          <div className="p-1">
            {visibleLoading && (
              <div
                className="mb-1 flex items-center gap-2 rounded-md px-2 py-1.5 text-xs"
                style={{
                  color: "var(--text-secondary)",
                  background: "var(--bg-surface)",
                }}
              >
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                <span>{loadingLabel}</span>
              </div>
            )}
            {activeTab === "pull_requests" && pullRequestMessage && (
              <div
                className="mb-1 rounded-md px-2 py-1.5 text-xs"
                style={{
                  color: "var(--text-secondary)",
                  background: "var(--bg-surface)",
                }}
              >
                {pullRequestMessage}
              </div>
            )}
            {visibleOptions.length === 0 ? (
              <div
                className="flex items-center justify-center py-6 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                {emptyLabel}
              </div>
            ) : (
              <div className="space-y-0.5">
                {visibleOptions.map((option) => {
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
