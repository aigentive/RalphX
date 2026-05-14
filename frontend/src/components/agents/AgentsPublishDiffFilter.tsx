/**
 * AgentsPublishDiffFilter
 *
 * Popover-based diff mode selector for the inline diff view.
 * Trigger: shows current mode label ("Uncommitted (N files)" / "abc1234 – message").
 * Content:
 *   - Radio "Uncommitted (N files)"
 *   - Collapsible "SPECIFIC COMMIT" section: filter input + commit radio list.
 *
 * v1: "All commits" mode is intentionally omitted — no cumulative base..HEAD diff API exists.
 *
 * Modeled on AgentsSidebar.tsx filter popover pattern.
 * WKWebView CSS: longhand background-color / border-color with literal or shallow-chain tokens.
 */

import { useState, useMemo } from "react";
import { Check, ChevronDown, GitCommit, SlidersHorizontal } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import type { Commit as DiffViewerCommit } from "@/components/diff";

export type DiffFilterMode = "uncommitted" | string; // string = commit SHA

export interface AgentsPublishDiffFilterProps {
  mode: DiffFilterMode;
  uncommittedCount: number;
  commits: DiffViewerCommit[];
  onModeChange: (mode: DiffFilterMode) => void;
}

// ── Local filter components (mirror of AgentsSidebar pattern) ──────────────

function FilterSectionLabel({ children }: { children: string }) {
  return (
    <div
      className="px-1.5 text-[0.625rem] font-semibold uppercase leading-none tracking-[0.12em]"
      style={{ color: "var(--text-muted)" }}
    >
      {children}
    </div>
  );
}

function FilterRadioRow({
  selected,
  onClick,
  testId,
  children,
}: {
  selected: boolean;
  onClick: () => void;
  testId: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      data-testid={testId}
      onClick={onClick}
      className="flex w-full items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left text-xs outline-none transition-colors duration-[120ms] hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
      style={{ color: selected ? "var(--text-primary)" : "var(--text-muted)" }}
    >
      <span className="truncate">{children}</span>
      <Check
        className="h-3.5 w-3.5 shrink-0"
        aria-hidden="true"
        style={{
          color: selected ? "var(--accent-primary)" : "var(--text-muted)",
          opacity: selected ? 1 : 0.35,
        }}
      />
    </button>
  );
}

// ── Trigger label helpers ──────────────────────────────────────────────────

function getTriggerLabel(
  mode: DiffFilterMode,
  uncommittedCount: number,
  commits: DiffViewerCommit[],
): string {
  if (mode === "uncommitted") {
    return `Uncommitted (${uncommittedCount} ${uncommittedCount === 1 ? "file" : "files"})`;
  }
  const commit = commits.find((c) => c.sha === mode);
  if (commit) {
    const truncated =
      commit.message.length > 40 ? `${commit.message.slice(0, 40)}…` : commit.message;
    return `${commit.shortSha} – ${truncated}`;
  }
  return mode.slice(0, 7);
}

// ── Main component ─────────────────────────────────────────────────────────

export function AgentsPublishDiffFilter({
  mode,
  uncommittedCount,
  commits,
  onModeChange,
}: AgentsPublishDiffFilterProps) {
  const [open, setOpen] = useState(false);
  const [commitSearch, setCommitSearch] = useState("");
  const [commitsOpen, setCommitsOpen] = useState(mode !== "uncommitted");

  const filteredCommits = useMemo(() => {
    if (!commitSearch.trim()) return commits;
    const q = commitSearch.toLowerCase();
    return commits.filter(
      (c) =>
        c.shortSha.toLowerCase().includes(q) ||
        c.message.toLowerCase().includes(q) ||
        c.author.toLowerCase().includes(q),
    );
  }, [commits, commitSearch]);

  const handleSelect = (next: DiffFilterMode) => {
    onModeChange(next);
    setOpen(false);
  };

  const triggerLabel = getTriggerLabel(mode, uncommittedCount, commits);

  return (
    <Popover open={open} onOpenChange={setOpen} modal={false}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-testid="diff-filter-trigger"
          className={cn(
            "inline-flex h-7 min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent bg-transparent",
            "px-2 text-[0.7188rem] font-medium text-[var(--text-muted)] transition-colors duration-[120ms]",
            "outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]",
            "focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]",
          )}
        >
          <SlidersHorizontal className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span className="truncate">{triggerLabel}</span>
          <ChevronDown
            className="h-3 w-3 shrink-0 transition-transform duration-[120ms]"
            aria-hidden="true"
            style={{ transform: open ? "rotate(180deg)" : "rotate(0deg)" }}
          />
        </button>
      </PopoverTrigger>

      <PopoverContent
        align="start"
        className="w-72 px-1.5 py-2.5"
        data-testid="diff-filter-popover"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
          boxShadow: "var(--shadow-sm)",
        }}
      >
        <div className="space-y-3 text-xs">
          {/* VIEW section */}
          <div className="space-y-1">
            <FilterSectionLabel>View</FilterSectionLabel>
            <FilterRadioRow
              selected={mode === "uncommitted"}
              onClick={() => handleSelect("uncommitted")}
              testId="diff-filter-option-uncommitted"
            >
              Uncommitted ({uncommittedCount} {uncommittedCount === 1 ? "file" : "files"})
            </FilterRadioRow>
          </div>

          {/* SPECIFIC COMMIT section */}
          {commits.length > 0 && (
            <Collapsible
              open={commitsOpen}
              onOpenChange={setCommitsOpen}
              data-testid="diff-filter-commits-section"
            >
              <CollapsibleTrigger
                data-testid="diff-filter-commits-section-trigger"
                className="flex w-full items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left transition-colors duration-[120ms] outline-none hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
              >
                <span
                  className="text-[0.625rem] font-semibold uppercase tracking-[0.12em]"
                  style={{ color: "var(--text-muted)" }}
                >
                  Specific Commit
                </span>
                <span
                  className="inline-flex shrink-0 items-center gap-1.5 text-[0.625rem] font-medium"
                  style={{ color: "var(--text-secondary)" }}
                >
                  <span>{commits.length}</span>
                  <ChevronDown
                    className="h-3 w-3 transition-transform duration-[120ms]"
                    aria-hidden="true"
                    style={{ transform: commitsOpen ? "rotate(180deg)" : "rotate(0deg)" }}
                  />
                </span>
              </CollapsibleTrigger>

              <CollapsibleContent className="pt-1">
                <div className="mb-1 px-1">
                  <Input
                    data-testid="diff-filter-commit-search"
                    placeholder="Filter commits…"
                    value={commitSearch}
                    onChange={(e) => setCommitSearch(e.target.value)}
                    className="h-7 text-xs"
                  />
                </div>

                <div className="max-h-44 space-y-0.5 overflow-y-auto">
                  {filteredCommits.map((commit) => (
                    <button
                      key={commit.sha}
                      type="button"
                      data-testid={`diff-filter-commit-${commit.sha}`}
                      onClick={() => handleSelect(commit.sha)}
                      className="flex w-full items-center gap-2 rounded-[4px] px-1.5 py-1.5 text-left transition-colors duration-[120ms] outline-none hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
                      style={{
                        backgroundColor:
                          mode === commit.sha ? "var(--accent-muted)" : "transparent",
                      }}
                    >
                      <GitCommit
                        className="h-3 w-3 shrink-0"
                        aria-hidden="true"
                        style={{ color: "var(--text-muted)" }}
                      />
                      <span
                        className="font-mono text-[0.6875rem] shrink-0"
                        style={{
                          color:
                            mode === commit.sha ? "var(--accent-primary)" : "var(--text-muted)",
                        }}
                      >
                        {commit.shortSha}
                      </span>
                      <span
                        className="min-w-0 truncate text-[0.75rem]"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        {commit.message}
                      </span>
                      {mode === commit.sha && (
                        <Check
                          className="ml-auto h-3 w-3 shrink-0"
                          aria-hidden="true"
                          style={{ color: "var(--accent-primary)" }}
                        />
                      )}
                    </button>
                  ))}
                  {filteredCommits.length === 0 && (
                    <p
                      className="px-1.5 py-2 text-center text-xs"
                      style={{ color: "var(--text-muted)" }}
                    >
                      No matching commits
                    </p>
                  )}
                </div>
              </CollapsibleContent>
            </Collapsible>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}
