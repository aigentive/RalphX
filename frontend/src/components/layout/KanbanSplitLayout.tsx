/**
 * KanbanSplitLayout - Split-screen layout for Kanban view
 *
 * Provides a split layout with:
 * - Left side: Kanban board + task detail overlay (when selected)
 * - Right side: selected-task chat only when an agent chat is available
 *
 * This layout intentionally does not render a project/main chat pane.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useUiStore } from "@/stores/uiStore";
import { IntegratedChatPanel } from "@/components/Chat/IntegratedChatPanel";
import { TaskDetailOverlay } from "@/components/tasks/TaskDetailOverlay";
import { TaskCreationOverlay } from "@/components/tasks/TaskCreationOverlay";
import { ResizeHandle, CHAT_PANEL_DEFAULT_WIDTH, CHAT_PANEL_MIN_WIDTH } from "@/components/ui/ResizeHandle";
import { useTaskChatAvailability } from "@/hooks/useTaskChatAvailability";

const MAX_CHAT_WIDTH = 600;
const TASK_CHAT_WIDTH_STORAGE_KEY = "ralphx-kanban-task-chat-width";

// ============================================================================
// Main Component
// ============================================================================

interface KanbanSplitLayoutProps {
  children: React.ReactNode;
  projectId: string;
  /** Optional footer to render at the bottom of the left section (e.g., ExecutionControlBar) */
  footer?: React.ReactNode;
}

export function KanbanSplitLayout({ children, projectId, footer }: KanbanSplitLayoutProps) {
  const selectedTaskId = useUiStore((s) => s.selectedTaskId);
  const setSelectedTaskId = useUiStore((s) => s.setSelectedTaskId);
  const taskCreationContext = useUiStore((s) => s.taskCreationContext);
  const showTaskChat = useTaskChatAvailability(projectId);
  const containerRef = useRef<HTMLDivElement>(null);
  const [isResizing, setIsResizing] = useState(false);
  const [chatPanelWidth, setChatPanelWidth] = useState(() => {
    const saved = localStorage.getItem(TASK_CHAT_WIDTH_STORAGE_KEY);
    if (saved) {
      const parsed = parseInt(saved, 10);
      if (!Number.isNaN(parsed) && parsed >= CHAT_PANEL_MIN_WIDTH && parsed <= MAX_CHAT_WIDTH) {
        return parsed;
      }
    }
    return CHAT_PANEL_DEFAULT_WIDTH;
  });

  useEffect(() => {
    localStorage.setItem(TASK_CHAT_WIDTH_STORAGE_KEY, chatPanelWidth.toString());
  }, [chatPanelWidth]);

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  }, []);

  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const newWidth = rect.right - e.clientX;
      setChatPanelWidth(Math.max(CHAT_PANEL_MIN_WIDTH, Math.min(MAX_CHAT_WIDTH, newWidth)));
    };

    const handleMouseUp = () => setIsResizing(false);

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);

    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isResizing]);

  return (
    <div
      ref={containerRef}
      data-testid="kanban-split-layout"
      className="flex h-full overflow-hidden"
      style={{ backgroundColor: "var(--app-content-bg)" }}
    >
      {/* Left Section - Kanban board */}
      <div
        data-testid="kanban-split-left"
        className="relative flex-1 flex flex-col overflow-hidden min-w-0"
        style={{
          transition: isResizing ? "none" : "width 150ms ease-out",
        }}
      >
        {/* Kanban Board */}
        <div className="flex-1 overflow-hidden">
          {children}
        </div>

        {/* Footer (e.g., ExecutionControlBar) */}
        {footer && (
          <div className="flex-shrink-0">
            {footer}
          </div>
        )}

        {/* Task Detail Overlay */}
        {selectedTaskId && (
          <TaskDetailOverlay
            projectId={projectId}
            footer={footer}
            constrainContent={!showTaskChat}
          />
        )}

        {/* Task Creation Overlay */}
        {taskCreationContext && <TaskCreationOverlay projectId={projectId} />}
      </div>

      {showTaskChat && (
        <ResizeHandle
          isResizing={isResizing}
          onMouseDown={handleResizeStart}
          testId="kanban-task-chat-resize-handle"
        />
      )}

      {showTaskChat && (
        <div
          data-testid="kanban-task-chat-panel"
          className="flex flex-col overflow-hidden shrink-0 border-l"
          style={{
            width: `${chatPanelWidth}px`,
            borderColor: "var(--app-content-border)",
            transition: isResizing ? "none" : "width 150ms ease-out",
          }}
        >
          <IntegratedChatPanel
            projectId={projectId}
            onClose={() => setSelectedTaskId(null)}
            autoFocusInput={false}
          />
        </div>
      )}
    </div>
  );
}
