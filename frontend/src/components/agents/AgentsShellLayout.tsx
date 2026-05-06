import { useMemo, type ComponentProps, type ReactNode, type Ref } from "react";

import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsSidebar } from "./AgentsSidebar";
import { AgentsSidebarVisibilityProvider } from "./AgentsSidebarVisibilityProvider";

type AgentsSidebarShellProps = Omit<ComponentProps<typeof AgentsSidebar>, "onCollapse">;

interface AgentsShellLayoutProps {
  children: ReactNode;
  isSidebarCollapsed: boolean;
  isSidebarOverlayOpen: boolean;
  onCloseSidebarOverlay: () => void;
  onToggleSidebarCollapse: () => void;
  sidebarProps: AgentsSidebarShellProps;
  sidebarWidth: number;
  splitContainerRef: Ref<HTMLDivElement>;
  suppressSidebarTransition: { current: boolean };
}

export function AgentsShellLayout({
  children,
  isSidebarCollapsed,
  isSidebarOverlayOpen,
  onCloseSidebarOverlay,
  onToggleSidebarCollapse,
  sidebarProps,
  sidebarWidth,
  splitContainerRef,
  suppressSidebarTransition,
}: AgentsShellLayoutProps) {
  const sidebarTransitionStyle =
    suppressSidebarTransition.current
      ? "none"
      : "width 300ms ease";
  const visibilityValue = useMemo(
    () => ({ isCollapsed: isSidebarCollapsed, onToggle: onToggleSidebarCollapse }),
    [isSidebarCollapsed, onToggleSidebarCollapse],
  );
  return (
    <TooltipProvider delayDuration={300}>
      <AgentsSidebarVisibilityProvider value={visibilityValue}>
      <section
        className="h-full min-h-0 w-full flex overflow-hidden"
        style={{ backgroundColor: "var(--app-content-bg)" }}
        data-testid="agents-view"
      >
        {isSidebarCollapsed && !isSidebarOverlayOpen && (
          <div
            role="button"
            aria-label="Open sidebar"
            tabIndex={0}
            data-testid="agents-sidebar-toggle-strip"
            onClick={onToggleSidebarCollapse}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onToggleSidebarCollapse();
              }
            }}
            className="shrink-0 cursor-pointer transition-colors duration-150"
            style={{
              width: 16,
              backgroundColor: "var(--app-sidebar-bg)",
              borderRightColor: "var(--app-sidebar-border)",
              borderRightStyle: "solid",
              borderRightWidth: "1px",
            }}
            onMouseEnter={(event) => {
              event.currentTarget.style.backgroundColor = "var(--overlay-weak)";
            }}
            onMouseLeave={(event) => {
              event.currentTarget.style.backgroundColor = "var(--app-sidebar-bg)";
            }}
          />
        )}

        {isSidebarOverlayOpen && (
          <div
            aria-hidden="true"
            onClick={onCloseSidebarOverlay}
            data-testid="agents-sidebar-overlay-backdrop"
            style={{
              position: "fixed",
              inset: 0,
              top: 48,
              backgroundColor: "var(--overlay-scrim)",
              zIndex: 34,
            }}
          />
        )}

        {!isSidebarOverlayOpen && (
          <div
            style={{
              width: isSidebarCollapsed ? 0 : sidebarWidth,
              minWidth: isSidebarCollapsed ? 0 : sidebarWidth,
              flexShrink: 0,
              overflow: "hidden",
              transition: sidebarTransitionStyle,
              display: isSidebarCollapsed ? "none" : undefined,
            }}
            aria-hidden={isSidebarCollapsed ? "true" : undefined}
            data-testid="agents-sidebar-container"
          >
            <AgentsSidebar {...sidebarProps} onCollapse={onToggleSidebarCollapse} />
          </div>
        )}

        {isSidebarOverlayOpen && (
          <div
            className="plan-browser-slide-in"
            style={{
              position: "fixed",
              top: 48,
              left: 0,
              height: "calc(100vh - 48px)",
              width: sidebarWidth || 340,
              zIndex: 35,
            }}
          >
            <AgentsSidebar {...sidebarProps} onCollapse={onCloseSidebarOverlay} />
          </div>
        )}

        <div
          ref={splitContainerRef}
          className="relative flex-1 min-w-0 h-full flex overflow-hidden"
          style={{ backgroundColor: "var(--app-content-bg)" }}
          data-testid="agents-split-container"
        >
          {children}
        </div>
      </section>
      </AgentsSidebarVisibilityProvider>
    </TooltipProvider>
  );
}
