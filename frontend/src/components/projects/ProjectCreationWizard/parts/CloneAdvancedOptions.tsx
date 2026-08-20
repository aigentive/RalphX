/**
 * CloneAdvancedOptions - collapsible depth/single-branch/submodule options
 * inside CloneStep, mirroring GitSettingsSection's advanced-settings pattern.
 * Labels are plain language only - no git flags rendered on screen. Defaults
 * (all off/empty) map to no CLI flags being sent.
 */

import { ChevronDown, Settings } from "lucide-react";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

export interface CloneAdvancedOptionsProps {
  depth: string;
  onDepthChange: (value: string) => void;
  singleBranch: boolean;
  onSingleBranchChange: (value: boolean) => void;
  recurseSubmodules: boolean;
  onRecurseSubmodulesChange: (value: boolean) => void;
  isCreating: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function CloneAdvancedOptions({
  depth,
  onDepthChange,
  singleBranch,
  onSingleBranchChange,
  recurseSubmodules,
  onRecurseSubmodulesChange,
  isCreating,
  open,
  onOpenChange,
}: CloneAdvancedOptionsProps) {
  return (
    <Collapsible open={open} onOpenChange={onOpenChange}>
      <CollapsibleTrigger
        data-testid="clone-advanced-options-trigger"
        className="flex items-center gap-2 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
      >
        <Settings className="h-3 w-3" />
        <span>Advanced Options</span>
        <ChevronDown className={cn("h-3 w-3 transition-transform", open && "rotate-180")} />
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-3 space-y-3 animate-in slide-in-from-top-2 fade-in duration-200">
        <div className="space-y-1.5">
          <Label htmlFor="clone-depth-input" className="text-sm font-medium text-[var(--text-secondary)]">
            Only download recent history
          </Label>
          <Input
            id="clone-depth-input"
            data-testid="clone-depth-input"
            type="number"
            min={1}
            value={depth}
            onChange={(e) => onDepthChange(e.target.value)}
            placeholder="Full history"
            disabled={isCreating}
            className={cn(
              "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
              isCreating && "opacity-50"
            )}
          />
          <p className="text-xs text-[var(--text-muted)]">
            Number of recent commits to download. Leave blank for the full history.
          </p>
        </div>

        <div className="flex items-center justify-between gap-3">
          <div className="space-y-0.5">
            <Label htmlFor="clone-single-branch-toggle" className="text-sm font-medium text-[var(--text-secondary)]">
              Only download the selected branch
            </Label>
            <p className="text-xs text-[var(--text-muted)]">Skips downloading every other branch.</p>
          </div>
          <Switch
            id="clone-single-branch-toggle"
            data-testid="clone-single-branch-toggle"
            checked={singleBranch}
            onCheckedChange={onSingleBranchChange}
            disabled={isCreating}
          />
        </div>

        <div className="flex items-center justify-between gap-3">
          <div className="space-y-0.5">
            <Label htmlFor="clone-submodules-toggle" className="text-sm font-medium text-[var(--text-secondary)]">
              Include linked sub-projects
            </Label>
            <p className="text-xs text-[var(--text-muted)]">Also downloads any submodules this repository uses.</p>
          </div>
          <Switch
            id="clone-submodules-toggle"
            data-testid="clone-submodules-toggle"
            checked={recurseSubmodules}
            onCheckedChange={onRecurseSubmodulesChange}
            disabled={isCreating}
          />
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}
