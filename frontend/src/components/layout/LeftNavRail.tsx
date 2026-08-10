/**
 * LeftNavRail — narrow vertical app navigation.
 *
 * Hosts the primary app views and the Settings entry in a compact
 * icon-and-label rail.
 */

import { Bug, Settings } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useTicketingProviders } from "@/hooks/useTicketing";
import { useGranolaIntegration } from "@/hooks/useGranolaIntegration";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { hasValidTicketingProvider } from "@/lib/ticketing-provider-state";
import { useProjectStore } from "@/stores/projectStore";
import { ALL_NAV_ITEMS } from "./nav-items";
import { BrandMark } from "./BrandMark";
import type { AppView } from "@/types/app-view";

export const LEFT_NAV_RAIL_WIDTH = 72;

interface LeftNavRailProps {
  currentView: AppView;
  onViewChange: (view: AppView) => void;
  onViewWarmUp?: (view: AppView) => void;
  onOpenSettings?: () => void;
  onOpenIssueReport?: () => void;
  /** Hide primary view items (e.g. during welcome screen). Settings stays. */
  hideViews?: boolean;
}

interface RailItemProps {
  view?: AppView;
  label: string;
  icon: React.ElementType;
  shortcut?: string | undefined;
  isActive: boolean;
  onClick: () => void;
  onWarmUp?: () => void;
  testId?: string;
}

function RailItem({
  label,
  icon: Icon,
  shortcut,
  isActive,
  onClick,
  onWarmUp,
  testId,
}: RailItemProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onClick}
          onPointerEnter={onWarmUp}
          onFocus={onWarmUp}
          aria-label={label}
          aria-current={isActive ? "page" : undefined}
          data-theme-button-skip
          data-testid={testId}
          className={cn(
            "relative grid h-[44px] w-[44px] place-items-center rounded-[10px] border p-0",
            "transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] active:scale-[0.98]",
            "outline-none ring-0 focus:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]",
            isActive
              ? "bg-[var(--bg-hover)] text-[var(--nav-rail-active-color)]"
              : "bg-transparent text-[var(--nav-rail-inactive-color)] hover:bg-[var(--bg-hover)] hover:text-[var(--nav-rail-active-color)]"
          )}
          style={{
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
  onViewWarmUp,
  onOpenSettings,
  onOpenIssueReport,
  hideViews = false,
}: LeftNavRailProps) {
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const { data: featureFlags } = useFeatureFlags();
  const { data: ticketingProviders } = useTicketingProviders(
    activeProjectId ?? undefined,
    { enabled: !hideViews },
  );
  const { connected: hasGranolaDashboardProvider } = useGranolaIntegration();
  const hasTicketingDashboardProvider = hasValidTicketingProvider(ticketingProviders);
  const isRemoteEnvironment = useIsRemoteEnvironment();

  const visibleItems = hideViews
    ? []
    : ALL_NAV_ITEMS.filter((item) => item.visible(featureFlags));
  const dashboardViews = new Set<AppView>(["ticketing", "github", "granola"]);
  const primaryItems = visibleItems.filter((item) => !dashboardViews.has(item.view));
  const dashboardItems = visibleItems.filter((item) => {
    if (item.view === "github") {
      return !isRemoteEnvironment;
    }
    if (item.view === "granola") {
      return hasGranolaDashboardProvider;
    }
    if (item.view === "ticketing") {
      return hasTicketingDashboardProvider;
    }
    return false;
  });

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
          {primaryItems.map(({ view, label, icon, shortcut }) => (
            <RailItem
              key={view}
              view={view}
              label={label}
              icon={icon}
              shortcut={shortcut}
              isActive={currentView === view}
              onClick={() => onViewChange(view)}
              onWarmUp={() => onViewWarmUp?.(view)}
              testId={`nav-${view}`}
            />
          ))}
          {dashboardItems.length > 0 && (
            <>
              <div
                className="my-2 h-px w-7 shrink-0"
                style={{ backgroundColor: "var(--border-default)" }}
                aria-hidden="true"
                data-testid="nav-dashboard-separator"
              />
              <div
                className="flex flex-col items-center gap-1"
                role="group"
                aria-label="Dashboard"
              >
                {dashboardItems.map(({ view, label, icon, shortcut }) => (
                  <RailItem
                    key={view}
                    view={view}
                    label={label}
                    icon={icon}
                    shortcut={shortcut}
                    isActive={currentView === view}
                    onClick={() => onViewChange(view)}
                    onWarmUp={() => onViewWarmUp?.(view)}
                    testId={`nav-${view}`}
                  />
                ))}
              </div>
            </>
          )}
        </nav>
      )}

      <div className="mt-auto flex flex-col items-center gap-1">
        {onOpenIssueReport && (
          <RailItem
            label="Report Issue"
            icon={Bug}
            isActive={false}
            onClick={onOpenIssueReport}
            testId="nav-report-issue"
          />
        )}
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
