/**
 * Navigation - Main view navigation bar
 */

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  LayoutGrid,
  Network,
  Lightbulb,
  Bot,
  Puzzle,
  Activity,
  SlidersHorizontal,
  TrendingUp,
  Users,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useTeamStore, selectHasAnyActiveTeam, selectTotalTeammateCount } from "@/stores/teamStore";
import { useProjectStore } from "@/stores/projectStore";
import { useProjectStats } from "@/hooks/useProjectStats";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { ALL_NAV_ITEMS } from "./nav-items";
import type { ViewType } from "@/types/chat";

interface NavItemConfig {
  view: ViewType;
  label: string;
  icon: React.ElementType;
  shortcut?: string;
  visible: (flags: FeatureFlags, taskCount: number) => boolean;
}

// Unified nav items with visibility predicates.
// Order matches the main navigation shortcut map: ⌘1 through ⌘5.
const ALL_NAV_ITEMS: NavItemConfig[] = [
  {
    view: "agents",
    label: "Agents",
    icon: Bot,
    shortcut: "⌘1",
    visible: () => true,
  },
  {
    view: "ideation",
    label: "Ideation",
    icon: Lightbulb,
    shortcut: "⌘2",
    visible: () => true,
  },
  {
    view: "graph",
    label: "Graph",
    icon: Network,
    shortcut: "⌘3",
    visible: () => true,
  },
  {
    view: "kanban",
    label: "Kanban",
    icon: LayoutGrid,
    shortcut: "⌘4",
    visible: () => true,
  },
  {
    view: "insights",
    label: "Insights",
    icon: TrendingUp,
    shortcut: "⌘5",
    visible: (_flags, taskCount) => taskCount >= 10,
  },
  {
    view: "extensibility",
    label: "Extensibility",
    icon: Puzzle,
    visible: (flags) => flags.extensibilityPage,
  },
  {
    view: "activity",
    label: "Activity",
    icon: Activity,
    visible: (flags) => flags.activityPage,
  },
];

interface NavigationProps {
  currentView: ViewType;
  onViewChange: (view: ViewType) => void;
  onOpenSettings?: () => void;
  /** When true, hide view tabs + team indicator and only render Settings.
   *  Used during the first-run welcome screen. */
  hideViews?: boolean;
}

function NavItem({
  view,
  label,
  icon: Icon,
  shortcut,
  currentView,
  onViewChange,
}: {
  view: ViewType;
  label: string;
  icon: React.ElementType;
  shortcut: string | undefined;
  currentView: ViewType;
  onViewChange: (view: ViewType) => void;
}) {
  const isActive = currentView === view;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onViewChange(view)}
          className={cn(
            "gap-2 h-8 transition-all duration-150 active:scale-[0.98]",
            isActive ? "px-3" : "px-2 xl:px-3"
          )}
          style={{
            background: isActive
              ? "var(--accent-muted)"
              : "transparent",
            border: isActive ? "1px solid var(--accent-border)" : "1px solid transparent",
            color: isActive ? "var(--accent-primary)" : "var(--text-muted)",
          }}
          data-testid={`nav-${view}`}
          aria-current={isActive ? "page" : undefined}
        >
          <Icon className="w-[18px] h-[18px] flex-shrink-0" />
          <span className={cn(
            "text-sm font-medium whitespace-nowrap",
            isActive ? "inline" : "hidden xl:inline"
          )}>
            {label}
          </span>
        </Button>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="text-xs">
        {label}
        {shortcut && <kbd className="ml-1 opacity-70">{shortcut}</kbd>}
      </TooltipContent>
    </Tooltip>
  );
}

export function Navigation({ currentView, onViewChange, onOpenSettings, hideViews = false }: NavigationProps) {
  const hasActiveTeam = useTeamStore(selectHasAnyActiveTeam);
  const teammateCount = useTeamStore(selectTotalTeammateCount);
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const { data: stats } = useProjectStats(activeProjectId ?? undefined);
  const { data: featureFlags } = useFeatureFlags();

  const taskCount = stats?.taskCount ?? 0;
  const visibleItems = hideViews
    ? []
    : ALL_NAV_ITEMS.filter((item) => item.visible(featureFlags, taskCount));

  return (
    <nav
      className="flex items-center gap-1"
      role="navigation"
      aria-label="Main views"
      style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
    >
      {visibleItems.map(({ view, label, icon, shortcut }) => (
        <NavItem
          key={view}
          view={view}
          label={label}
          icon={icon}
          shortcut={shortcut}
          currentView={currentView}
          onViewChange={onViewChange}
        />
      ))}

      {/* Settings button — opens modal overlay */}
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            onClick={onOpenSettings}
            className="gap-2 h-8 px-2 xl:px-3 transition-all duration-150 active:scale-[0.98]"
            style={{
              background: "transparent",
              border: "1px solid transparent",
              color: "var(--text-muted)",
            }}
            data-testid="nav-settings"
          >
            <SlidersHorizontal className="w-[18px] h-[18px] flex-shrink-0" />
            <span className="text-sm font-medium whitespace-nowrap hidden xl:inline">Settings</span>
          </Button>
        </TooltipTrigger>
        <TooltipContent side="bottom" className="text-xs">
          Settings <kbd className="ml-1 opacity-70">⌘,</kbd>
        </TooltipContent>
      </Tooltip>

      {/* Team active indicator */}
      {hasActiveTeam && !hideViews && (
        <div
          className="flex items-center gap-1.5 h-7 px-2.5 rounded-full ml-1"
          style={{
            background: "var(--accent-muted)",
            border: "1px solid var(--accent-border)",
          }}
        >
          <Users className="w-3.5 h-3.5" style={{ color: "var(--accent-primary)" }} />
          <span className="text-xs font-medium" style={{ color: "var(--accent-primary)" }}>
            {teammateCount}
          </span>
        </div>
      )}
    </nav>
  );
}
