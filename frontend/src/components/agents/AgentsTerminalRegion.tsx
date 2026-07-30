import {
  Suspense,
  useCallback,
  useEffect,
  useRef,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Terminal as TerminalIcon } from "lucide-react";

import type { AgentConversationWorkspace } from "@/api/chat";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";
import { formatBranchDisplay } from "@/lib/branch-utils";
import {
  safelyUnlistenTauri,
  type TauriUnlistenFn,
} from "@/lib/tauri-listener-cleanup";

import { useResolvedAgentArtifactState } from "./agentArtifactState";
import { useAfterPaintMounted } from "./agentDeferredFrame";
import { compactTerminalPath } from "./agentTerminalPaths";
import { preloadAgentTerminalDrawer } from "./agentTerminalPreload";
import {
  AGENT_TERMINAL_COLLAPSED_HEIGHT,
  AGENT_TERMINAL_DEFAULT_HEIGHT,
  useAgentTerminalStore,
  type AgentTerminalCachedStatus,
  type AgentTerminalDock,
  type AgentTerminalPlacement,
} from "./agentTerminalStore";

const LazyAgentTerminalDrawer = lazyWithRetry(() =>
  preloadAgentTerminalDrawer().then((module) => ({ default: module.AgentTerminalDrawer })),
);

const TERMINAL_DOCK_DROP_LABELS: Record<AgentTerminalDock, string> = {
  chat: "Drop under chat",
  panel: "Drop in side panel",
};
const TERMINAL_DRAG_END_CLEANUP_DELAY_MS = 120;
const TERMINAL_DOCK_DRAG_LEAVE_CLEAR_DELAY_MS = 80;
type DragEventWithOptionalTransfer = { dataTransfer?: DataTransfer };
interface NativeDockDragPosition {
  x: number;
  y: number;
}
interface NativeDockDragPayload {
  type: string;
  position?: NativeDockDragPosition;
}
interface NativeDockDragEvent {
  payload: NativeDockDragPayload;
}

function getOptionalDataTransfer(
  event: ReactDragEvent<HTMLElement>,
): DataTransfer | undefined {
  return (event as unknown as DragEventWithOptionalTransfer).dataTransfer;
}

function isNativePositionInsideElement(
  position: NativeDockDragPosition | undefined,
  element: HTMLElement | null,
) {
  if (!position || !element) {
    return false;
  }

  const rect = element.getBoundingClientRect();
  return (
    position.x >= rect.left &&
    position.x <= rect.right &&
    position.y >= rect.top &&
    position.y <= rect.bottom
  );
}

function AgentTerminalLoadingShell({
  height,
  expanded,
  terminalStatus,
  workspace,
  dockElement,
  onToggleExpanded,
}: {
  height: number;
  expanded: boolean;
  terminalStatus: AgentTerminalCachedStatus;
  workspace: AgentConversationWorkspace;
  dockElement: HTMLElement | null;
  onToggleExpanded: () => void;
}) {
  const branchLabel = formatBranchDisplay(workspace.branchName).short;
  const displayCwd = compactTerminalPath(workspace.worktreePath);
  const statusLabel =
    expanded ? "Opening" : terminalStatus === "closed" ? "Closed" : terminalStatus;
  const shell = (
    <div
      className="relative shrink-0 overflow-hidden border-t"
      style={{
        height: expanded ? height : AGENT_TERMINAL_COLLAPSED_HEIGHT,
        backgroundColor: "var(--bg-base)",
        borderColor: "var(--overlay-weak)",
        boxShadow: "0 -16px 36px var(--shadow-card)",
      }}
      data-testid="agent-terminal-loading-shell"
    >
      <div
        className="flex h-9 cursor-pointer select-none items-center gap-2 border-b px-3 text-xs"
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        aria-label={expanded ? "Collapse terminal panel" : "Expand terminal panel"}
        data-testid="agent-terminal-loading-shell-header"
        onClick={onToggleExpanded}
        onKeyDown={(event: ReactKeyboardEvent<HTMLDivElement>) => {
          if (event.key !== "Enter" && event.key !== " ") {
            return;
          }
          event.preventDefault();
          onToggleExpanded();
        }}
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--overlay-faint)",
          color: "var(--text-secondary)",
        }}
      >
        <TerminalIcon
          className="h-3.5 w-3.5 shrink-0"
          style={{ color: "var(--accent-primary)" }}
        />
        <span className="font-medium" style={{ color: "var(--text-primary)" }}>
          Terminal
        </span>
        <span
          className="h-1 w-1 rounded-full"
          style={{ backgroundColor: "var(--text-muted)" }}
        />
        <span>{statusLabel}</span>
        <span className="min-w-0 truncate font-mono" style={{ color: "var(--text-muted)" }}>
          {branchLabel}
        </span>
        <span
          className="hidden min-w-0 truncate font-mono md:inline"
          style={{ color: "var(--text-muted)" }}
        >
          {displayCwd}
        </span>
      </div>
      {expanded ? (
        <div className="px-3 py-2 font-mono text-xs" style={{ color: "var(--text-muted)" }}>
          Starting terminal...
        </div>
      ) : null}
    </div>
  );

  return dockElement ? createPortal(shell, dockElement) : shell;
}

function AgentTerminalArchivedShell({
  height,
  expanded,
  reason,
  workspace,
  dockElement,
  onToggleExpanded,
}: {
  height: number;
  expanded: boolean;
  reason: string;
  workspace: AgentConversationWorkspace;
  dockElement: HTMLElement | null;
  onToggleExpanded: () => void;
}) {
  const branchLabel = formatBranchDisplay(workspace.branchName).short;
  const displayCwd = compactTerminalPath(workspace.worktreePath);
  const shell = (
    <div
      className="relative shrink-0 overflow-hidden border-t"
      style={{
        height: expanded ? height : AGENT_TERMINAL_COLLAPSED_HEIGHT,
        backgroundColor: "var(--bg-base)",
        borderColor: "var(--overlay-weak)",
        borderStyle: "solid",
        borderTopWidth: "1px",
        boxShadow: "0 -16px 36px var(--shadow-card)",
      }}
      data-testid="agent-terminal-archived-shell"
    >
      <div
        className="flex h-9 cursor-pointer select-none items-center gap-2 border-b px-3 text-xs"
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        aria-label={
          expanded ? "Collapse archived terminal panel" : "Expand archived terminal panel"
        }
        data-testid="agent-terminal-archived-shell-header"
        onClick={onToggleExpanded}
        onKeyDown={(event: ReactKeyboardEvent<HTMLDivElement>) => {
          if (event.key !== "Enter" && event.key !== " ") {
            return;
          }
          event.preventDefault();
          onToggleExpanded();
        }}
        style={{
          backgroundColor: "var(--bg-surface)",
          borderBottomWidth: "1px",
          borderColor: "var(--overlay-faint)",
          borderStyle: "solid",
          color: "var(--text-secondary)",
        }}
      >
        <TerminalIcon
          className="h-3.5 w-3.5 shrink-0"
          style={{ color: "var(--text-muted)" }}
        />
        <span className="font-medium" style={{ color: "var(--text-primary)" }}>
          Terminal
        </span>
        <span
          className="h-1 w-1 rounded-full"
          style={{ backgroundColor: "var(--text-muted)" }}
        />
        <span>Archived</span>
        <span className="min-w-0 truncate font-mono" style={{ color: "var(--text-muted)" }}>
          {branchLabel}
        </span>
        <span
          className="hidden min-w-0 truncate font-mono md:inline"
          style={{ color: "var(--text-muted)" }}
        >
          {displayCwd}
        </span>
      </div>
      {expanded ? (
        <div
          className="space-y-2 px-3 py-3 text-xs"
          style={{ color: "var(--text-secondary)" }}
        >
          <p>{reason}</p>
          <p style={{ color: "var(--text-muted)" }}>
            No terminal process is running for this archived workspace.
          </p>
        </div>
      ) : null}
    </div>
  );

  return dockElement ? createPortal(shell, dockElement) : shell;
}

interface AgentsTerminalPresentationInput {
  conversationId: string | null;
  workspace: AgentConversationWorkspace | null;
  terminalArchivedReason: string | null;
  terminalUnavailableReason: string | null;
  hasAutoOpenArtifacts: boolean;
}

function useAgentTerminalPresentation({
  conversationId,
  workspace,
  terminalUnavailableReason,
  hasAutoOpenArtifacts,
}: AgentsTerminalPresentationInput) {
  const isExpanded = useAgentTerminalStore((state) =>
    conversationId ? state.openByConversationId[conversationId] ?? false : false,
  );
  const height = useAgentTerminalStore((state) =>
    conversationId
      ? state.heightByConversationId[conversationId] ?? AGENT_TERMINAL_DEFAULT_HEIGHT
      : AGENT_TERMINAL_DEFAULT_HEIGHT,
  );
  const placement = useAgentTerminalStore((state) => state.placement);
  const { artifactPaneOpen } = useResolvedAgentArtifactState(
    conversationId,
    hasAutoOpenArtifacts,
  );
  const canRender = Boolean(conversationId && workspace && !terminalUnavailableReason);
  const dockTarget =
    artifactPaneOpen && (placement === "panel" || placement === "auto")
      ? "panel"
      : "chat";

  return {
    canRender,
    isExpanded,
    height,
    placement,
    dockTarget,
    artifactPaneOpen,
  };
}

interface AgentsTerminalDockHostProps extends AgentsTerminalPresentationInput {
  dock: "chat" | "panel";
  setDockElement: (element: HTMLDivElement | null) => void;
}

export function AgentsTerminalDockHost({
  dock,
  conversationId,
  workspace,
  terminalArchivedReason,
  terminalUnavailableReason,
  hasAutoOpenArtifacts,
  setDockElement,
}: AgentsTerminalDockHostProps) {
  const { canRender, isExpanded, height, dockTarget, artifactPaneOpen } =
    useAgentTerminalPresentation({
      conversationId,
      workspace,
      terminalArchivedReason,
      terminalUnavailableReason,
      hasAutoOpenArtifacts,
    });
  const draggingConversationId = useAgentTerminalStore(
    (state) => state.draggingConversationId,
  );
  const dragOverDock = useAgentTerminalStore((state) => state.dragOverDock);
  const setTerminalPlacement = useAgentTerminalStore((state) => state.setPlacement);
  const setDragOverDock = useAgentTerminalStore((state) => state.setDragOverDock);
  const clearDragState = useAgentTerminalStore((state) => state.clearDragState);
  const dockHostRef = useRef<HTMLDivElement | null>(null);
  const dragLeaveClearTimerRef = useRef<number | null>(null);

  const setDockHostElement = useCallback(
    (element: HTMLDivElement | null) => {
      dockHostRef.current = element;
      setDockElement(element);
    },
    [setDockElement],
  );

  const cancelDragLeaveClear = useCallback(() => {
    if (dragLeaveClearTimerRef.current !== null) {
      window.clearTimeout(dragLeaveClearTimerRef.current);
      dragLeaveClearTimerRef.current = null;
    }
  }, []);

  const scheduleDragLeaveClear = useCallback(() => {
    cancelDragLeaveClear();
    dragLeaveClearTimerRef.current = window.setTimeout(() => {
      dragLeaveClearTimerRef.current = null;
      const terminalState = useAgentTerminalStore.getState();
      if (terminalState.dragOverDock === dock) {
        terminalState.setDragOverDock(null);
      }
    }, TERMINAL_DOCK_DRAG_LEAVE_CLEAR_DELAY_MS);
  }, [cancelDragLeaveClear, dock]);

  const isDocumentDragEventOverDock = useCallback((event: DragEvent) => {
    const host = dockHostRef.current;
    if (!host) {
      return false;
    }

    const elementAtPointer =
      typeof document.elementFromPoint === "function"
        ? document.elementFromPoint(event.clientX, event.clientY)
        : null;
    if (elementAtPointer && host.contains(elementAtPointer)) {
      return true;
    }

    const rect = host.getBoundingClientRect();
    return (
      event.clientX >= rect.left &&
      event.clientX <= rect.right &&
      event.clientY >= rect.top &&
      event.clientY <= rect.bottom
    );
  }, []);

  useEffect(
    () => () => {
      cancelDragLeaveClear();
    },
    [cancelDragLeaveClear],
  );

  const isVisible = dockTarget === dock;
  const resolvedDockHeight = isExpanded ? height : AGENT_TERMINAL_COLLAPSED_HEIGHT;
  const targetAvailable = dock === "chat" || artifactPaneOpen;
  const isDropTarget =
    canRender &&
    draggingConversationId === conversationId &&
    targetAvailable &&
    dockTarget !== dock;
  const isDragOver = isDropTarget && dragOverDock === dock;

  const activateDropTarget = (event: ReactDragEvent<HTMLDivElement>) => {
    if (!isDropTarget) {
      return;
    }
    cancelDragLeaveClear();
    event.preventDefault();
    const dataTransfer = getOptionalDataTransfer(event);
    if (dataTransfer) {
      dataTransfer.dropEffect = "move";
    }
    setDragOverDock(dock);
  };

  const handleDragLeave = () => {
    if (dragOverDock !== dock) {
      return;
    }
    scheduleDragLeaveClear();
  };

  const handleDrop = (event: ReactDragEvent<HTMLDivElement>) => {
    if (!isDropTarget) {
      return;
    }
    event.preventDefault();
    cancelDragLeaveClear();
    setTerminalPlacement(dock);
    clearDragState();
  };

  useEffect(() => {
    if (!isDropTarget) {
      return;
    }

    const handleDocumentDragMove = (event: DragEvent) => {
      if (!isDocumentDragEventOverDock(event)) {
        if (useAgentTerminalStore.getState().dragOverDock === dock) {
          scheduleDragLeaveClear();
        }
        return;
      }

      event.preventDefault();
      cancelDragLeaveClear();
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = "move";
      }
      setDragOverDock(dock);
    };

    const handleDocumentDrop = (event: DragEvent) => {
      if (!isDocumentDragEventOverDock(event)) {
        return;
      }

      event.preventDefault();
      cancelDragLeaveClear();
      setTerminalPlacement(dock);
      clearDragState();
    };

    document.addEventListener("dragenter", handleDocumentDragMove, true);
    document.addEventListener("dragover", handleDocumentDragMove, true);
    document.addEventListener("drop", handleDocumentDrop, true);
    return () => {
      document.removeEventListener("dragenter", handleDocumentDragMove, true);
      document.removeEventListener("dragover", handleDocumentDragMove, true);
      document.removeEventListener("drop", handleDocumentDrop, true);
    };
  }, [
    cancelDragLeaveClear,
    clearDragState,
    dock,
    isDocumentDragEventOverDock,
    isDropTarget,
    scheduleDragLeaveClear,
    setDragOverDock,
    setTerminalPlacement,
  ]);

  useEffect(() => {
    if (!isDropTarget) {
      return;
    }

    let unlisten: TauriUnlistenFn | undefined;
    let cancelled = false;

    const releaseNativeDragListener = () => {
      const dispose = unlisten;
      unlisten = undefined;
      safelyUnlistenTauri(dispose, "terminal dock drag listener");
    };

    const setupNativeDragListener = async () => {
      try {
        const webview = getCurrentWebview();
        const dispose = await webview.onDragDropEvent((event: NativeDockDragEvent) => {
          const { payload } = event;
          const isInside = isNativePositionInsideElement(
            payload.position,
            dockHostRef.current,
          );

          if (payload.type === "over" || payload.type === "enter") {
            if (isInside) {
              cancelDragLeaveClear();
              setDragOverDock(dock);
              return;
            }

            if (useAgentTerminalStore.getState().dragOverDock === dock) {
              scheduleDragLeaveClear();
            }
            return;
          }

          if (payload.type === "drop") {
            if (isInside) {
              cancelDragLeaveClear();
              setTerminalPlacement(dock);
              clearDragState();
              return;
            }

            if (useAgentTerminalStore.getState().dragOverDock === dock) {
              scheduleDragLeaveClear();
            }
            return;
          }

          if (useAgentTerminalStore.getState().dragOverDock === dock) {
            scheduleDragLeaveClear();
          }
        });

        if (cancelled) {
          safelyUnlistenTauri(dispose, "terminal dock drag listener");
          return;
        }
        unlisten = dispose;
      } catch (error) {
        console.error("Failed to set up terminal dock drag listener:", error);
      }
    };

    void setupNativeDragListener();

    return () => {
      cancelled = true;
      releaseNativeDragListener();
    };
  }, [
    cancelDragLeaveClear,
    clearDragState,
    dock,
    isDropTarget,
    scheduleDragLeaveClear,
    setDragOverDock,
    setTerminalPlacement,
  ]);

  if (!canRender) {
    return null;
  }

  return (
    <div
      ref={setDockHostElement}
      className="shrink-0 overflow-hidden"
      style={{
        height: isVisible || isDropTarget ? resolvedDockHeight : 0,
        opacity: isVisible || isDropTarget ? 1 : 0,
        pointerEvents: isVisible || isDropTarget ? "auto" : "none",
        transition: "none",
      }}
      data-testid={dock === "panel" ? "agent-terminal-host-panel" : "agent-terminal-host-chat"}
      data-terminal-drop-target={isDropTarget ? "true" : "false"}
      aria-label={isDropTarget ? TERMINAL_DOCK_DROP_LABELS[dock] : undefined}
      onDragLeave={handleDragLeave}
      onDragEnter={activateDropTarget}
      onDragOver={activateDropTarget}
      onDrop={handleDrop}
    >
      {isDropTarget ? (
        <div
          className="flex h-full items-center justify-center px-3 text-xs font-medium"
          data-testid={`agent-terminal-drop-target-${dock}`}
          style={{
            backgroundColor: isDragOver
              ? "color-mix(in srgb, var(--accent-primary) 12%, var(--bg-surface) 88%)"
              : "color-mix(in srgb, var(--accent-primary) 6%, var(--bg-surface) 94%)",
            borderColor: isDragOver ? "var(--accent-primary)" : "var(--accent-border)",
            borderStyle: "dashed",
            borderWidth: "2px",
            color: isDragOver ? "var(--accent-primary)" : "var(--text-secondary)",
          }}
        >
          <span
            className="rounded-full px-3 py-1"
            style={{
              backgroundColor: "var(--bg-elevated)",
              borderColor: isDragOver ? "var(--accent-primary)" : "var(--accent-border)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            {TERMINAL_DOCK_DROP_LABELS[dock]}
          </span>
        </div>
      ) : null}
    </div>
  );
}

interface AgentsTerminalRegionProps extends AgentsTerminalPresentationInput {
  chatDockElement: HTMLElement | null;
  panelDockElement: HTMLElement | null;
  onOpenArtifactTab: (conversationId: string, tab: AgentArtifactTab) => void;
}

export function AgentsTerminalRegion({
  conversationId,
  workspace,
  terminalArchivedReason,
  terminalUnavailableReason,
  hasAutoOpenArtifacts,
  chatDockElement,
  panelDockElement,
  onOpenArtifactTab,
}: AgentsTerminalRegionProps) {
  const {
    canRender,
    isExpanded,
    height,
    placement,
    dockTarget,
    artifactPaneOpen,
  } = useAgentTerminalPresentation({
    conversationId,
    workspace,
    terminalArchivedReason,
    terminalUnavailableReason,
    hasAutoOpenArtifacts,
  });
  const shouldMountInteractiveTerminal = canRender && !terminalArchivedReason;
  const contentMounted = useAfterPaintMounted(shouldMountInteractiveTerminal);
  const terminalStatus = useAgentTerminalStore((state) =>
    conversationId ? state.statusByConversationId[conversationId] ?? "closed" : "closed",
  );
  const setTerminalHeight = useAgentTerminalStore((state) => state.setHeight);
  const setTerminalOpen = useAgentTerminalStore((state) => state.setOpen);
  const setTerminalPlacement = useAgentTerminalStore((state) => state.setPlacement);
  const setDraggingConversation = useAgentTerminalStore(
    (state) => state.setDraggingConversation,
  );
  const dragOverDock = useAgentTerminalStore((state) => state.dragOverDock);
  const clearDragState = useAgentTerminalStore((state) => state.clearDragState);
  const dragEndCleanupTimerRef = useRef<number | null>(null);

  const cancelDragEndCleanup = useCallback(() => {
    if (dragEndCleanupTimerRef.current !== null) {
      window.clearTimeout(dragEndCleanupTimerRef.current);
      dragEndCleanupTimerRef.current = null;
    }
  }, []);

  const handlePlacementChange = useCallback(
    (nextPlacement: AgentTerminalPlacement) => {
      setTerminalPlacement(nextPlacement);
      if (nextPlacement === "panel" && conversationId && !artifactPaneOpen) {
        onOpenArtifactTab(conversationId, "publish");
      }
    },
    [artifactPaneOpen, conversationId, onOpenArtifactTab, setTerminalPlacement],
  );
  const handlePlacementDragStart = useCallback(() => {
    cancelDragEndCleanup();
    if (conversationId) {
      setDraggingConversation(conversationId);
    }
  }, [cancelDragEndCleanup, conversationId, setDraggingConversation]);
  const handlePlacementDragEnd = useCallback(() => {
    cancelDragEndCleanup();
    if (dragOverDock) {
      setTerminalPlacement(dragOverDock);
      clearDragState();
      return;
    }

    dragEndCleanupTimerRef.current = window.setTimeout(() => {
      dragEndCleanupTimerRef.current = null;
      clearDragState();
    }, TERMINAL_DRAG_END_CLEANUP_DELAY_MS);
  }, [cancelDragEndCleanup, clearDragState, dragOverDock, setTerminalPlacement]);
  useEffect(
    () => () => {
      cancelDragEndCleanup();
    },
    [cancelDragEndCleanup],
  );

  if (!canRender || !conversationId || !workspace) {
    return null;
  }

  const dockElement = dockTarget === "panel" ? panelDockElement : chatDockElement;
  const handleTerminalShellToggle = () => {
    setTerminalOpen(conversationId, !isExpanded);
  };

  if (terminalArchivedReason) {
    return (
      <AgentTerminalArchivedShell
        height={height}
        expanded={isExpanded}
        reason={terminalArchivedReason}
        workspace={workspace}
        dockElement={dockElement}
        onToggleExpanded={handleTerminalShellToggle}
      />
    );
  }

  if (!contentMounted) {
    return (
      <AgentTerminalLoadingShell
        height={height}
        expanded={isExpanded}
        terminalStatus={terminalStatus}
        workspace={workspace}
        dockElement={dockElement}
        onToggleExpanded={handleTerminalShellToggle}
      />
    );
  }

  return (
    <Suspense
      fallback={
        <AgentTerminalLoadingShell
          height={height}
          expanded={isExpanded}
          terminalStatus={terminalStatus}
          workspace={workspace}
          dockElement={dockElement}
          onToggleExpanded={handleTerminalShellToggle}
        />
      }
    >
      <LazyAgentTerminalDrawer
        key={conversationId}
        conversationId={conversationId}
        workspace={workspace}
        height={height}
        expanded={isExpanded}
        onHeightChange={(nextHeight) => setTerminalHeight(conversationId, nextHeight)}
        onExpand={() => setTerminalOpen(conversationId, true)}
        onCollapse={() => setTerminalOpen(conversationId, false)}
        placement={placement}
        onPlacementChange={handlePlacementChange}
        onPlacementDragStart={handlePlacementDragStart}
        onPlacementDragEnd={handlePlacementDragEnd}
        dockElement={dockElement}
      />
    </Suspense>
  );
}
