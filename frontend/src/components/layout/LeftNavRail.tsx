/**
 * LeftNavRail — narrow vertical app navigation.
 *
 * Hosts the same primary views as the legacy top-bar Navigation
 * (Agents, Ideation, Graph, Kanban, Insights, plus feature-flagged items)
 * and the Settings entry, in a compact icon-and-label rail.
 */

import { Settings } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useProjectStats } from "@/hooks/useProjectStats";
import { useProjectStore } from "@/stores/projectStore";
import { ALL_NAV_ITEMS } from "./nav-items";
import { BrandMark } from "./BrandMark";
import type { ViewType } from "@/types/chat";

export const LEFT_NAV_RAIL_WIDTH = 72;

interface LeftNavRailProps {
  currentView: ViewType;
  onViewChange: (view: ViewType) => void;
  onOpenSettings?: () => void;
  /** Hide primary view items (e.g. during welcome screen). Settings stays. */
  hideViews?: boolean;
}

interface RailItemProps {
  view?: ViewType;
  label: string;
  icon: React.ElementType;
  shortcut?: string | undefined;
  isActive: boolean;
  onClick: () => void;
  testId?: string;
}

function RailItem({
  label,
  icon: Icon,
  shortcut,
  isActive,
  onClick,
  testId,
}: RailItemProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onClick}
          aria-label={label}
          aria-current={isActive ? "page" : undefined}
          data-theme-button-skip
          data-testid={testId}
          className={cn(
            "relative grid h-[44px] w-[44px] place-items-center rounded-[10px] border p-0",
            "transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] active:scale-[0.98]",
            "outline-none ring-0 focus:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]",
            !isActive && "hover:bg-[var(--bg-hover)]",
            isActive
              ? "text-[var(--nav-rail-active-color)]"
              : "text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
          )}
          style={{
            color: isActive ? "var(--nav-rail-active-color)" : "var(--nav-rail-inactive-color)",
            backgroundColor: isActive ? "var(--bg-hover)" : "transparent",
            borderColor: "transparent",
            borderStyle: "solid",
            borderWidth: "1px",
            boxShadow: isActive ? "var(--nav-rail-active-shadow)" : "none",
          }}
        >
          {isActive && (
            <span
              aria-hidden="true"
              className="left-nav-rail__active-border absolute left-[-14px] top-1/2 h-[18px] w-0.5 -translate-y-1/2 rounded-r-sm"
            />
          )}
          <Icon className="h-[22px] w-[22px] flex-shrink-0" strokeWidth={1.8} />
          <span className="sr-only">{label}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent side="right" className="text-xs">
        {label}
        {shortcut && <kbd className="ml-1 opacity-70">{shortcut}</kbd>}
      </TooltipContent>
    </Tooltip>
  );
}

export function LeftNavRail({
  currentView,
  onViewChange,
  onOpenSettings,
  hideViews = false,
}: LeftNavRailProps) {
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const { data: stats } = useProjectStats(activeProjectId ?? undefined);
  const { data: featureFlags } = useFeatureFlags();

  const taskCount = stats?.taskCount ?? 0;
  const visibleItems = hideViews
    ? []
    : ALL_NAV_ITEMS.filter((item) => item.visible(featureFlags, taskCount));

  return (
    <aside
      className="flex shrink-0 flex-col items-center gap-1 overflow-hidden border-r px-0 pb-3 pt-[14px]"
      style={{
        width: LEFT_NAV_RAIL_WIDTH,
        backgroundColor: "var(--app-rail-bg)",
        borderRightColor: "var(--app-rail-border)",
        borderRightStyle: "solid",
        borderRightWidth: "1px",
        WebkitAppRegion: "no-drag",
      } as React.CSSProperties}
      role="navigation"
      aria-label="Primary"
      data-testid="left-nav-rail"
    >
      <div
        className="grid h-[44px] w-[44px] select-none place-items-center"
        data-testid="left-nav-brand"
        title="RalphX"
      >
        <BrandMark />
      </div>

      <div
        className="mb-3 mt-[14px] h-px w-7 shrink-0"
        style={{ backgroundColor: "var(--border-default)" }}
        aria-hidden="true"
      />

      {!hideViews && (
        <nav className="flex flex-col items-center gap-1">
          {visibleItems.map(({ view, label, icon, shortcut }) => (
            <RailItem
              key={view}
              view={view}
              label={label}
              icon={icon}
              shortcut={shortcut}
              isActive={currentView === view}
              onClick={() => onViewChange(view)}
              testId={`nav-${view}`}
            />
          ))}
        </nav>
      )}

      <div className="mt-auto flex flex-col items-center gap-1">
        <RailItem
          label="Settings"
          icon={Settings}
          shortcut="⌘,"
          isActive={false}
          onClick={() => onOpenSettings?.()}
          testId="nav-settings"
        />
      </div>
    </aside>
  );
}
