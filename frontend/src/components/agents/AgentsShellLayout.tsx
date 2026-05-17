import { useRef, type ComponentProps, type MouseEvent as ReactMouseEvent, type ReactNode, type Ref } from "react";
import { Menu } from "lucide-react";

import { ResizeHandle } from "@/components/ui/ResizeHandle";
import { TooltipProvider } from "@/components/ui/tooltip";
import { withAlpha } from "@/lib/theme-colors";

import { AgentsSidebar } from "./AgentsSidebar";
import { useAgentsSidebarResize } from "./useAgentsSidebarResize";

type AgentsSidebarShellProps = Omit<
  ComponentProps<typeof AgentsSidebar>,
  "isVisible" | "onCollapse"
>;

interface AgentsShellLayoutProps {
  children: ReactNode;
  footer?: ReactNode;
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
  footer,
  isSidebarCollapsed,
  isSidebarOverlayOpen,
  onCloseSidebarOverlay,
  onToggleSidebarCollapse,
  sidebarProps,
  sidebarWidth,
  splitContainerRef,
  suppressSidebarTransition,
}: AgentsShellLayoutProps) {
  const sidebarContainerRef = useRef<HTMLDivElement | null>(null);
  const {
    handleSidebarResizeReset,
    handleSidebarResizeStart,
    isSidebarResizing,
    userSidebarWidth,
  } = useAgentsSidebarResize(sidebarContainerRef);
  const effectiveSidebarWidth =
    !isSidebarCollapsed && userSidebarWidth !== null && sidebarWidth > 0
      ? userSidebarWidth
      : sidebarWidth;
  const showSidebarResizeHandle =
    !isSidebarCollapsed && !isSidebarOverlayOpen && sidebarWidth > 0;
  const sidebarTransitionStyle =
    suppressSidebarTransition.current || isSidebarResizing
      ? "none"
      : "width 300ms ease";
  const onSidebarResizeStart = (event: ReactMouseEvent) => {
    handleSidebarResizeStart(event);
  };
  const onSidebarResizeReset = (event: ReactMouseEvent) => {
    handleSidebarResizeReset(event);
  };
  return (
    <TooltipProvider delayDuration={300}>
      <section
        className="h-full min-h-0 w-full flex overflow-hidden"
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
            className="flex items-center justify-center shrink-0 cursor-pointer transition-colors duration-150"
            style={{
              width: 36,
              background: withAlpha("var(--bg-surface)", 50),
              borderRight: "1px solid var(--overlay-faint)",
              color: "var(--text-muted)",
            }}
            onMouseEnter={(event) => {
              event.currentTarget.style.background = "var(--overlay-weak)";
              event.currentTarget.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(event) => {
              event.currentTarget.style.background = withAlpha("var(--bg-surface)", 50);
              event.currentTarget.style.color = "var(--text-muted)";
            }}
          >
            <Menu className="w-4 h-4" />
          </div>
        )}

        {isSidebarOverlayOpen && (
          <div
            aria-hidden="true"
            onClick={onCloseSidebarOverlay}
            data-testid="agents-sidebar-overlay-backdrop"
            style={{
              position: "fixed",
              inset: 0,
              top: 56,
              background: "var(--overlay-scrim)",
              zIndex: 34,
            }}
          />
        )}

        {!isSidebarOverlayOpen && (
          <div
            ref={sidebarContainerRef}
            style={{
              width: isSidebarCollapsed ? 0 : effectiveSidebarWidth,
              minWidth: isSidebarCollapsed ? 0 : effectiveSidebarWidth,
              flexShrink: 0,
              overflow: "hidden",
              transition: sidebarTransitionStyle,
              display: isSidebarCollapsed ? "none" : undefined,
            }}
            aria-hidden={isSidebarCollapsed ? "true" : undefined}
            data-testid="agents-sidebar-container"
          >
            <AgentsSidebar
              {...sidebarProps}
              isVisible={!isSidebarCollapsed}
              onCollapse={onToggleSidebarCollapse}
            />
          </div>
        )}

        {showSidebarResizeHandle && (
          <ResizeHandle
            isResizing={isSidebarResizing}
            onMouseDown={onSidebarResizeStart}
            onDoubleClick={onSidebarResizeReset}
            testId="agents-sidebar-resize-handle"
          />
        )}

        {isSidebarOverlayOpen && (
          <div
            className="plan-browser-slide-in"
            style={{
              position: "fixed",
              top: 56,
              left: 0,
              height: "calc(100vh - 56px)",
              width: sidebarWidth || 340,
              zIndex: 35,
            }}
          >
            <AgentsSidebar
              {...sidebarProps}
              isVisible={isSidebarOverlayOpen}
              onCollapse={onCloseSidebarOverlay}
            />
          </div>
        )}

        <div
          ref={splitContainerRef}
          className="relative flex-1 min-w-0 h-full flex flex-col overflow-hidden"
          data-testid="agents-split-container"
        >
          <div className="flex-1 min-h-0 flex overflow-hidden">
            {children}
          </div>
          {footer && (
            <div className="flex-shrink-0" data-testid="agents-footer">
              {footer}
            </div>
          )}
        </div>
      </section>
    </TooltipProvider>
  );
}
