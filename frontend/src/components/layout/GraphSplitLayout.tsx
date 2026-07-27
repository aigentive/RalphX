/**
 * GraphSplitLayout - Split-screen layout for Graph view
 *
 * Provides a split layout with:
 * - Left side: ReactFlow canvas (takes remaining space)
 * - Right side: FloatingTimeline at fixed 320px
 *
 * Agents owns task detail and task chat; this layout only hosts Graph and timeline.
 */

import { useEffect, useRef, useState } from "react";
import { useUiStore } from "@/stores/uiStore";
import { TaskCreationOverlay } from "@/components/tasks/TaskCreationOverlay";
import { SeparatorLine } from "@/components/ui/ResizeHandle";

// ============================================================================
// Constants
// ============================================================================

// Fixed timeline sidebar width (px) - non-resizable
const TIMELINE_SIDEBAR_WIDTH = 320;

const overlayAnimationStyles = `
@keyframes graphPanelSlideIn {
  from { transform: translateX(12px); opacity: 0; }
  to { transform: translateX(0); opacity: 1; }
}

@keyframes graphPanelSlideOut {
  from { transform: translateX(0); opacity: 1; }
  to { transform: translateX(12px); opacity: 0; }
}

.graph-panel-enter {
  animation: graphPanelSlideIn 220ms ease-out forwards;
}

.graph-panel-exit {
  animation: graphPanelSlideOut 200ms ease-in forwards;
}
`;

// ============================================================================
// Main Component
// ============================================================================

interface GraphSplitLayoutProps {
  /** ReactFlow canvas content */
  children: React.ReactNode;
  /** Project ID for context */
  projectId: string;
  /** Optional footer to render at the bottom of the left section (e.g., ExecutionControlBar) */
  footer?: React.ReactNode;
  /** Timeline content to show when no task is selected */
  timelineContent: React.ReactNode;
  /** Right panel mode */
  rightPanelMode: "split" | "overlay" | "hidden";
}

export function GraphSplitLayout({
  children,
  projectId,
  footer,
  timelineContent,
  rightPanelMode,
}: GraphSplitLayoutProps) {
  const taskCreationContext = useUiStore((s) => s.taskCreationContext);
  const [overlayVisible, setOverlayVisible] = useState(false);
  const [overlayExiting, setOverlayExiting] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const overlayExitTimeoutRef = useRef<number | null>(null);

  useEffect(() => {
    if (rightPanelMode === "overlay") {
      if (overlayExitTimeoutRef.current) {
        window.clearTimeout(overlayExitTimeoutRef.current);
        overlayExitTimeoutRef.current = null;
      }
      setOverlayVisible(true);
      setOverlayExiting(false);
      return;
    }

    if (!overlayVisible || overlayExiting) return;

    setOverlayExiting(true);
    overlayExitTimeoutRef.current = window.setTimeout(() => {
      setOverlayVisible(false);
      setOverlayExiting(false);
      overlayExitTimeoutRef.current = null;
    }, 200);
  }, [rightPanelMode, overlayVisible, overlayExiting]);

  useEffect(() => {
    return () => {
      if (overlayExitTimeoutRef.current) {
        window.clearTimeout(overlayExitTimeoutRef.current);
      }
    };
  }, []);

  const panelWidthPx = TIMELINE_SIDEBAR_WIDTH;
  const panelWidth = `${TIMELINE_SIDEBAR_WIDTH}px`;

  return (
    <div
      ref={containerRef}
      data-testid="graph-split-layout"
      className="flex h-full overflow-hidden"
      style={{ backgroundColor: "var(--app-content-bg)" }}
    >
      <style>{overlayAnimationStyles}</style>
      {/* Left Section - Graph canvas */}
      <div
        data-testid="graph-split-left"
        className="relative flex-1 flex flex-col overflow-hidden min-w-0"
        style={{ transition: "width 150ms ease-out" }}
      >
        {/* Graph Canvas */}
        <div className="flex-1 overflow-hidden relative">
          {children}
        </div>

        {/* Footer (e.g., ExecutionControlBar) */}
        {footer && (
          <div className="flex-shrink-0">
            {footer}
          </div>
        )}

        {/* Task Creation Overlay */}
        {taskCreationContext && <TaskCreationOverlay projectId={projectId} />}
      </div>

      {rightPanelMode === "split" && (
        <>
          <SeparatorLine />

          {/* Right Section - Timeline */}
          <div
            data-testid="graph-split-right"
            className="flex flex-col overflow-hidden shrink-0"
            style={{
              width: panelWidth,
              transition: "width 150ms ease-out",
            }}
          >
            {timelineContent}
          </div>
        </>
      )}

      {overlayVisible && (
        <div
          data-testid="graph-split-right-overlay"
          className={`fixed top-12 right-0 flex flex-col pointer-events-auto ${
            overlayExiting ? "graph-panel-exit" : "graph-panel-enter"
          }`}
          style={{
            width: `${panelWidthPx + 16}px`,
            minWidth: `${TIMELINE_SIDEBAR_WIDTH + 16}px`,
            bottom: "76px",
            zIndex: 40,
          }}
          >
            <div
            className="flex flex-col flex-1 overflow-hidden rounded-[10px]"
            style={{
              margin: "8px",
              background: "var(--bg-elevated)",
              border: "1px solid var(--border-subtle)",
              boxShadow: "var(--shadow-md)",
            }}
          >
            {timelineContent}
          </div>
        </div>
      )}
    </div>
  );
}
