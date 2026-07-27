import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import type { FitAddon } from "@xterm/addon-fit";
import type {
  Terminal as XTermTerminal,
  IDisposable,
  ITheme,
} from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  ChevronDown,
  PanelBottomClose,
  PanelBottomOpen,
  RefreshCw,
  Terminal as TerminalIcon,
  Trash2,
  X,
} from "lucide-react";

import {
  AGENT_TERMINAL_EVENT,
  AgentTerminalEventSchema,
  closeAgentTerminal,
  clearAgentTerminal,
  DEFAULT_AGENT_TERMINAL_ID,
  openAgentTerminal,
  resizeAgentTerminal,
  restartAgentTerminal,
  writeAgentTerminal,
  type AgentTerminalEvent,
  type AgentTerminalSnapshot,
} from "@/api/terminal";
import type { AgentConversationWorkspace } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatBranchDisplay } from "@/lib/branch-utils";
import type { Unsubscribe } from "@/lib/event-bus";
import { useEventBus } from "@/providers/EventProvider";
import {
  RALPHX_TERMINAL_DOCK_DRAG_TYPE,
  setRalphxTerminalDockDragActive,
} from "@/lib/internalDragTypes";
import { cn } from "@/lib/utils";
import { compactTerminalPath } from "./agentTerminalPaths";
import { loadAgentTerminalRuntime } from "./agentTerminalRuntime";
import {
  AGENT_TERMINAL_COLLAPSED_HEIGHT,
  useAgentTerminalStore,
  type AgentTerminalCachedStatus,
  type AgentTerminalPlacement,
} from "./agentTerminalStore";

interface AgentTerminalDrawerProps {
  conversationId: string;
  workspace: AgentConversationWorkspace;
  height: number;
  expanded: boolean;
  onHeightChange: (height: number) => void;
  onExpand: () => void;
  onCollapse: () => void;
  placement: AgentTerminalPlacement;
  onPlacementChange: (placement: AgentTerminalPlacement) => void;
  onPlacementDragStart?: () => void;
  onPlacementDragEnd?: () => void;
  dockElement: HTMLElement | null;
}

const TERMINAL_MIN_COLS = 80;
const TERMINAL_MIN_ROWS = 20;
const HEADER_DRAG_CLICK_SUPPRESSION_MS = 150;
const HEADER_DRAG_START_THRESHOLD_PX = 4;
type AgentTerminalDisplayStatus = AgentTerminalCachedStatus;

type DeferredFrameJob = { frame: number | null; timer: number | null };
type DragEventWithOptionalTransfer = { dataTransfer?: DataTransfer };
type HeaderDragStartPoint = { x: number; y: number };

function getOptionalDataTransfer(
  event: ReactDragEvent<HTMLElement>,
): DataTransfer | undefined {
  return (event as unknown as DragEventWithOptionalTransfer).dataTransfer;
}

function hasMovedBeyondHeaderDragThreshold(
  startPoint: HeaderDragStartPoint,
  clientX: number,
  clientY: number,
) {
  return (
    Math.abs(clientX - startPoint.x) >= HEADER_DRAG_START_THRESHOLD_PX ||
    Math.abs(clientY - startPoint.y) >= HEADER_DRAG_START_THRESHOLD_PX
  );
}

function cancelDeferredFrameJob(job: DeferredFrameJob | null) {
  if (!job) {
    return;
  }
  if (job.frame !== null) {
    window.cancelAnimationFrame(job.frame);
  }
  if (job.timer !== null) {
    window.clearTimeout(job.timer);
  }
}

function scheduleDeferredFrameJob(callback: () => void): DeferredFrameJob {
  const job: DeferredFrameJob = {
    frame: null,
    timer: null,
  };
  job.frame = window.requestAnimationFrame(() => {
    job.frame = null;
    job.timer = window.setTimeout(() => {
      job.timer = null;
      callback();
    }, 0);
  });
  return job;
}

export function AgentTerminalDrawer({
  conversationId,
  workspace,
  height,
  expanded,
  onHeightChange,
  onExpand,
  onCollapse,
  placement,
  onPlacementChange,
  onPlacementDragStart,
  onPlacementDragEnd,
  dockElement,
}: AgentTerminalDrawerProps) {
  const eventBus = useEventBus();
  const terminalId = DEFAULT_AGENT_TERMINAL_ID;
  const [portalRoot] = useState(() => {
    const element = document.createElement("div");
    element.style.width = "100%";
    return element;
  });
  const [hasDocked, setHasDocked] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<XTermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const dockMoveJobRef = useRef<DeferredFrameJob | null>(null);
  const hydrationCompleteRef = useRef(false);
  const bufferedEventsRef = useRef<AgentTerminalEvent[]>([]);
  const lastAppliedEventKeyRef = useRef<string | null>(null);
  const lastReportedSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const resizeReportTimerRef = useRef<number | null>(null);
  const writeQueueRef = useRef<Promise<void>>(Promise.resolve());
  const suppressNextHeaderClickRef = useRef(false);
  const headerDragStartPointRef = useRef<HeaderDragStartPoint | null>(null);
  const headerDragMovedRef = useRef(false);
  const headerDockDragActiveRef = useRef(false);
  const headerDragClickTimerRef = useRef<number | null>(null);
  const cachedStatus = useAgentTerminalStore(
    (state) => state.statusByConversationId[conversationId] ?? "closed",
  );
  const setCachedTerminalStatus = useAgentTerminalStore((state) => state.setStatus);
  const [status, setStatus] = useState<AgentTerminalDisplayStatus>(cachedStatus);
  const [cwd, setCwd] = useState(workspace.worktreePath);
  const [branchName, setBranchName] = useState(workspace.branchName);
  const [isFocused, setIsFocused] = useState(false);
  const [isRestarting, setIsRestarting] = useState(false);
  const [isClearing, setIsClearing] = useState(false);
  const [isHydrating, setIsHydrating] = useState(false);
  const [isTerminalStarted, setIsTerminalStarted] = useState(false);

  const branchLabel = useMemo(
    () => formatBranchDisplay(branchName).short,
    [branchName],
  );
  const displayCwd = useMemo(() => compactTerminalPath(cwd), [cwd]);
  const displayHeight = expanded ? height : AGENT_TERMINAL_COLLAPSED_HEIGHT;
  const shouldHydrateTerminal = hasDocked && (expanded || isTerminalStarted);
  const shouldRenderTerminalBody = expanded || isTerminalStarted;
  const statusLabel = isHydrating
    ? "Opening"
    : status === "closed"
      ? "Closed"
      : status;

  const terminalTheme = useMemo(() => readTerminalTheme(), []);

  useEffect(() => {
    setStatus(cachedStatus);
  }, [cachedStatus]);

  const updateStatus = useCallback(
    (nextStatus: AgentTerminalDisplayStatus) => {
      setStatus(nextStatus);
      setCachedTerminalStatus(conversationId, nextStatus);
    },
    [conversationId, setCachedTerminalStatus],
  );

  const fitTerminal = useCallback(() => {
    const terminal = terminalRef.current;
    const fitAddon = fitAddonRef.current;
    if (!terminal || !fitAddon || !containerRef.current) {
      return null;
    }

    try {
      fitAddon.fit();
      return {
        cols: Math.max(terminal.cols || 0, TERMINAL_MIN_COLS),
        rows: Math.max(terminal.rows || 0, TERMINAL_MIN_ROWS),
      };
    } catch {
      // xterm can throw when fitting while detached during fast route switches.
      return null;
    }
  }, []);

  const fitAndReportSize = useCallback(() => {
    const size = fitTerminal();
    if (!size) {
      return;
    }
    const lastReported = lastReportedSizeRef.current;
    if (
      lastReported &&
      lastReported.cols === size.cols &&
      lastReported.rows === size.rows
    ) {
      return;
    }
    lastReportedSizeRef.current = size;

    if (resizeReportTimerRef.current !== null) {
      window.clearTimeout(resizeReportTimerRef.current);
    }
    resizeReportTimerRef.current = window.setTimeout(() => {
      resizeReportTimerRef.current = null;
      void resizeAgentTerminal({
        conversationId,
        terminalId,
        cols: size.cols,
        rows: size.rows,
      }).catch(() => undefined);
    }, 80);
  }, [conversationId, fitTerminal, terminalId]);

  const cancelDockMove = useCallback(() => {
    cancelDeferredFrameJob(dockMoveJobRef.current);
    dockMoveJobRef.current = null;
  }, []);

  useEffect(
    () => () => {
      cancelDockMove();
      portalRoot.remove();
    },
    [cancelDockMove, portalRoot],
  );

  useLayoutEffect(() => {
    if (!dockElement) {
      return;
    }

    cancelDockMove();
    const currentDock = portalRoot.parentElement;
    if (currentDock === dockElement) {
      if (!hasDocked) {
        setHasDocked(true);
      }
      return;
    }

    if (!currentDock) {
      dockElement.appendChild(portalRoot);
      if (!hasDocked) {
        setHasDocked(true);
      }
      return;
    }

    portalRoot.parentElement?.removeChild(portalRoot);
    dockElement.appendChild(portalRoot);
    if (!hasDocked) {
      setHasDocked(true);
    }

    dockMoveJobRef.current = scheduleDeferredFrameJob(() => {
      dockMoveJobRef.current = null;
      fitAndReportSize();
    });
  }, [cancelDockMove, dockElement, fitAndReportSize, hasDocked, portalRoot]);

  const applySnapshot = useCallback(
    (snapshot: AgentTerminalSnapshot) => {
      updateStatus(snapshot.status);
      setIsTerminalStarted(true);
      setCwd(snapshot.cwd);
      setBranchName(snapshot.workspaceBranch);
    },
    [updateStatus],
  );

  const applyEvent = useCallback((event: AgentTerminalEvent) => {
    if (event.conversationId !== conversationId || event.terminalId !== terminalId) {
      return;
    }

    if (!hydrationCompleteRef.current) {
      bufferedEventsRef.current.push(event);
      return;
    }

    const eventKey = [
      event.type,
      event.updatedAt,
      event.data ?? "",
      event.message ?? "",
      event.exitCode ?? "",
      event.exitSignal ?? "",
    ].join(":");
    if (lastAppliedEventKeyRef.current === eventKey) {
      return;
    }
    lastAppliedEventKeyRef.current = eventKey;

    const terminal = terminalRef.current;
    if (event.cwd) {
      setCwd(event.cwd);
    }
    if (event.workspaceBranch) {
      setBranchName(event.workspaceBranch);
    }

    if (event.type === "started" || event.type === "restarted") {
      updateStatus("running");
      setIsTerminalStarted(true);
      if (event.type === "restarted") {
        terminal?.reset();
      }
      return;
    }

    if (event.type === "output" && event.data) {
      terminal?.write(event.data);
      return;
    }

    if (event.type === "cleared") {
      terminal?.clear();
      return;
    }

    if (event.type === "exited") {
      updateStatus("exited");
      terminal?.write("\r\n[terminal exited]\r\n");
      return;
    }

    if (event.type === "error") {
      updateStatus("error");
      if (event.message) {
        terminal?.write(`\r\n[terminal error] ${event.message}\r\n`);
      }
    }
  }, [conversationId, terminalId, updateStatus]);

  const showControlError = useCallback((error: unknown) => {
    const message = error instanceof Error ? error.message : "Terminal command failed";
    updateStatus("error");
    terminalRef.current?.write(`\r\n[terminal error] ${message}\r\n`);
  }, [updateStatus]);

  useEffect(() => {
    if (!shouldHydrateTerminal) {
      return;
    }
    const host = containerRef.current;
    if (!host) {
      return;
    }

    hydrationCompleteRef.current = false;
    bufferedEventsRef.current = [];
    setIsHydrating(true);
    setIsTerminalStarted(true);

    let disposed = false;
    let terminal: XTermTerminal | null = null;
    let dataDisposable: IDisposable | null = null;
    let resizeFrame: number | null = null;
    let initFrame: number | null = null;
    let initTimer: number | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let unsubscribe: Unsubscribe | null = null;

    const scheduleFit = () => {
      if (resizeFrame !== null) {
        window.cancelAnimationFrame(resizeFrame);
      }
      resizeFrame = window.requestAnimationFrame(() => {
        resizeFrame = null;
        fitAndReportSize();
      });
    };

    const start = async () => {
      const { Terminal, FitAddon } = await loadAgentTerminalRuntime();
      if (disposed) {
        return;
      }

      terminal = new Terminal({
        allowProposedApi: false,
        convertEol: true,
        cursorBlink: true,
        cursorStyle: "block",
        fontFamily:
          "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, Liberation Mono, monospace",
        fontSize: 12,
        lineHeight: 1.18,
        scrollback: 5_000,
        theme: terminalTheme,
      });
      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminal.open(host);
      terminalRef.current = terminal;
      fitAddonRef.current = fitAddon;
      setIsHydrating(false);

      unsubscribe = eventBus.subscribe<unknown>(AGENT_TERMINAL_EVENT, (payload) => {
        const parsed = AgentTerminalEventSchema.safeParse(payload);
        if (parsed.success) {
          applyEvent(parsed.data);
        }
      });
      await unsubscribe.ready;

      if (disposed) {
        return;
      }

      const initialSize = fitTerminal() ?? {
        cols: TERMINAL_MIN_COLS,
        rows: TERMINAL_MIN_ROWS,
      };
      lastReportedSizeRef.current = initialSize;
      const snapshot = await openAgentTerminal({
        conversationId,
        terminalId,
        cols: initialSize.cols,
        rows: initialSize.rows,
      });

      if (disposed) {
        return;
      }

      applySnapshot(snapshot);
      if (snapshot.history) {
        terminal.write(snapshot.history);
      }

      hydrationCompleteRef.current = true;
      const snapshotTime = Date.parse(snapshot.updatedAt);
      bufferedEventsRef.current
        .filter((item) => Number.isNaN(snapshotTime) || Date.parse(item.updatedAt) > snapshotTime)
        .forEach(applyEvent);
      bufferedEventsRef.current = [];

      dataDisposable = terminal.onData((data) => {
        const write = writeQueueRef.current
          .catch(() => undefined)
          .then(() =>
            writeAgentTerminal({
              conversationId,
              terminalId,
              data,
            }),
          )
          .catch(showControlError);
        writeQueueRef.current = write;
      });

      resizeObserver = new ResizeObserver(scheduleFit);
      resizeObserver.observe(host);
      terminal.focus();
    };

    initFrame = window.requestAnimationFrame(() => {
      initFrame = null;
      initTimer = window.setTimeout(() => {
        initTimer = null;
        void start().catch((error) => {
          if (disposed) {
            return;
          }
          setIsHydrating(false);
          updateStatus("error");
          const message = error instanceof Error ? error.message : "Failed to open terminal";
          terminalRef.current?.write(`\r\n[terminal error] ${message}\r\n`);
        });
      }, 0);
    });

    return () => {
      disposed = true;
      hydrationCompleteRef.current = false;
      if (initFrame !== null) {
        window.cancelAnimationFrame(initFrame);
      }
      if (initTimer !== null) {
        window.clearTimeout(initTimer);
      }
      if (resizeFrame !== null) {
        window.cancelAnimationFrame(resizeFrame);
      }
      if (resizeReportTimerRef.current !== null) {
        window.clearTimeout(resizeReportTimerRef.current);
        resizeReportTimerRef.current = null;
      }
      resizeObserver?.disconnect();
      dataDisposable?.dispose();
      unsubscribe?.();
      unsubscribe = null;
      terminal?.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [
    applyEvent,
    applySnapshot,
    conversationId,
    eventBus,
    fitTerminal,
    fitAndReportSize,
    shouldHydrateTerminal,
    showControlError,
    terminalId,
    terminalTheme,
    updateStatus,
  ]);

  useEffect(() => {
    if (!expanded || !isTerminalStarted) {
      return;
    }
    const job = scheduleDeferredFrameJob(fitAndReportSize);
    return () => cancelDeferredFrameJob(job);
  }, [expanded, fitAndReportSize, isTerminalStarted]);

  const hasTerminalInstance = useCallback(() => {
    if (!terminalRef.current) {
      return false;
    }
    return true;
  }, []);

  const handleClear = useCallback(async () => {
    if (!hasTerminalInstance()) {
      return;
    }
    setIsClearing(true);
    try {
      const snapshot = await clearAgentTerminal({
        conversationId,
        terminalId,
        deleteHistory: true,
      });
      terminalRef.current?.clear();
      applySnapshot(snapshot);
    } catch (error) {
      showControlError(error);
    } finally {
      setIsClearing(false);
    }
  }, [
    applySnapshot,
    conversationId,
    hasTerminalInstance,
    showControlError,
    terminalId,
  ]);

  const handleRestart = useCallback(async () => {
    const terminal = terminalRef.current;
    if (!terminal) {
      onExpand();
      return;
    }
    setIsRestarting(true);
    try {
      terminal.reset();
      const cols = Math.max(terminal.cols || 0, TERMINAL_MIN_COLS);
      const rows = Math.max(terminal.rows || 0, TERMINAL_MIN_ROWS);
      const snapshot = await restartAgentTerminal({
        conversationId,
        terminalId,
        cols,
        rows,
      });
      applySnapshot(snapshot);
    } catch (error) {
      showControlError(error);
    } finally {
      setIsRestarting(false);
    }
  }, [applySnapshot, conversationId, onExpand, showControlError, terminalId]);

  const handleClose = useCallback(() => {
    if (status === "closed" && !isHydrating) {
      onCollapse();
      return;
    }

    onCollapse();
    updateStatus("closed");
    setIsHydrating(false);
    setIsClearing(false);
    setIsRestarting(false);
    setIsTerminalStarted(false);
    hydrationCompleteRef.current = false;
    bufferedEventsRef.current = [];
    void closeAgentTerminal({ conversationId, terminalId }).catch(() => undefined);
  }, [conversationId, isHydrating, onCollapse, status, terminalId, updateStatus]);

  const handleExpandToggle = useCallback(() => {
    if (expanded) {
      onCollapse();
      return;
    }
    onExpand();
  }, [expanded, onCollapse, onExpand]);

  const clearHeaderDragClickTimer = useCallback(() => {
    if (headerDragClickTimerRef.current !== null) {
      window.clearTimeout(headerDragClickTimerRef.current);
      headerDragClickTimerRef.current = null;
    }
  }, []);

  const scheduleHeaderDragClickReset = useCallback(() => {
    clearHeaderDragClickTimer();
    headerDragClickTimerRef.current = window.setTimeout(() => {
      headerDragClickTimerRef.current = null;
      suppressNextHeaderClickRef.current = false;
    }, HEADER_DRAG_CLICK_SUPPRESSION_MS);
  }, [clearHeaderDragClickTimer]);

  const handleHeaderToggleClick = useCallback(() => {
    if (suppressNextHeaderClickRef.current) {
      suppressNextHeaderClickRef.current = false;
      clearHeaderDragClickTimer();
      return;
    }
    handleExpandToggle();
  }, [clearHeaderDragClickTimer, handleExpandToggle]);

  const handleHeaderKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (event.key !== "Enter" && event.key !== " ") {
        return;
      }
      event.preventDefault();
      handleExpandToggle();
    },
    [handleExpandToggle],
  );

  const handleHeaderPointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      headerDragStartPointRef.current = {
        x: event.clientX,
        y: event.clientY,
      };
      headerDragMovedRef.current = false;
    },
    [],
  );

  const handleHeaderPointerMove = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const startPoint = headerDragStartPointRef.current;
      if (!startPoint) {
        return;
      }
      if (
        hasMovedBeyondHeaderDragThreshold(
          startPoint,
          event.clientX,
          event.clientY,
        )
      ) {
        headerDragMovedRef.current = true;
      }
    },
    [],
  );

  const shouldStartHeaderDockDrag = useCallback(
    (event: ReactDragEvent<HTMLDivElement>) => {
      const startPoint = headerDragStartPointRef.current;
      if (!startPoint) {
        return true;
      }
      return (
        headerDragMovedRef.current ||
        hasMovedBeyondHeaderDragThreshold(startPoint, event.clientX, event.clientY)
      );
    },
    [],
  );

  const handleHeaderDragStart = useCallback(
    (event: ReactDragEvent<HTMLDivElement>) => {
      if (!shouldStartHeaderDockDrag(event)) {
        event.preventDefault();
        setRalphxTerminalDockDragActive(false);
        headerDockDragActiveRef.current = false;
        return;
      }
      suppressNextHeaderClickRef.current = true;
      headerDockDragActiveRef.current = true;
      setRalphxTerminalDockDragActive(true);
      const dataTransfer = getOptionalDataTransfer(event);
      if (dataTransfer) {
        dataTransfer.effectAllowed = "move";
        dataTransfer.setData(RALPHX_TERMINAL_DOCK_DRAG_TYPE, conversationId);
        dataTransfer.setData("text/plain", conversationId);
      }
      onPlacementDragStart?.();
    },
    [conversationId, onPlacementDragStart, shouldStartHeaderDockDrag],
  );

  const handleHeaderDragEnd = useCallback(() => {
    if (!headerDockDragActiveRef.current) {
      setRalphxTerminalDockDragActive(false);
      return;
    }
    headerDockDragActiveRef.current = false;
    setRalphxTerminalDockDragActive(false);
    onPlacementDragEnd?.();
    scheduleHeaderDragClickReset();
  }, [onPlacementDragEnd, scheduleHeaderDragClickReset]);

  useEffect(
    () => () => {
      headerDockDragActiveRef.current = false;
      setRalphxTerminalDockDragActive(false);
      clearHeaderDragClickTimer();
    },
    [clearHeaderDragClickTimer],
  );

  const handleResizeStart = useCallback(
    (event: ReactMouseEvent<HTMLButtonElement>) => {
      event.preventDefault();
      const startY = event.clientY;
      const startHeight = height;

      const handleMouseMove = (moveEvent: MouseEvent) => {
        onHeightChange(startHeight + (startY - moveEvent.clientY));
      };
      const handleMouseUp = () => {
        window.removeEventListener("mousemove", handleMouseMove);
        window.removeEventListener("mouseup", handleMouseUp);
      };

      window.addEventListener("mousemove", handleMouseMove);
      window.addEventListener("mouseup", handleMouseUp);
    },
    [height, onHeightChange],
  );

  if (!hasDocked) {
    return null;
  }

  const terminalSessionUnavailable = status === "closed" && !isHydrating;

  return createPortal(
    <div
      className={cn(
        "relative shrink-0 overflow-hidden border-t",
        isFocused && "border-t-2",
      )}
      style={{
        height: displayHeight,
        backgroundColor: "var(--bg-base)",
        borderColor: isFocused ? "var(--accent-border)" : "var(--overlay-weak)",
        boxShadow: "0 -16px 36px var(--shadow-card)",
      }}
      data-testid="agent-terminal-drawer"
      onFocusCapture={() => setIsFocused(true)}
      onBlurCapture={() => setIsFocused(false)}
    >
      <button
        type="button"
        className="absolute inset-x-0 top-0 z-10 h-2 cursor-ns-resize"
        aria-label="Resize terminal"
        onMouseDown={handleResizeStart}
      />

      <div
        className="flex h-9 items-center justify-between gap-3 border-b px-3"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--overlay-faint)",
        }}
      >
        <div
          className="flex min-w-0 flex-1 cursor-grab select-none items-center gap-2 text-xs active:cursor-grabbing"
          role="button"
          tabIndex={0}
          draggable
          aria-expanded={expanded}
          aria-label={expanded ? "Collapse terminal panel" : "Expand terminal panel"}
          data-testid="agent-terminal-header"
          onPointerDown={handleHeaderPointerDown}
          onPointerMove={handleHeaderPointerMove}
          onClick={handleHeaderToggleClick}
          onKeyDown={handleHeaderKeyDown}
          onDragStart={handleHeaderDragStart}
          onDragEnd={handleHeaderDragEnd}
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
          <span className="shrink-0 capitalize" style={{ color: "var(--text-secondary)" }}>
            {statusLabel}
          </span>
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

        <div className="flex shrink-0 items-center gap-1">
          <TerminalPlacementButton
            placement={placement}
            onPlacementChange={onPlacementChange}
          />
          {expanded ? (
            <>
              <TerminalIconButton
                label="Clear terminal"
                onClick={() => void handleClear()}
                disabled={isClearing || isHydrating || terminalSessionUnavailable}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </TerminalIconButton>
              <TerminalIconButton
                label={
                  terminalSessionUnavailable
                    ? "Open terminal"
                    : "Start fresh terminal session"
                }
                onClick={() => void handleRestart()}
                disabled={isRestarting || isHydrating}
              >
                <RefreshCw className={cn("h-3.5 w-3.5", isRestarting && "animate-spin")} />
              </TerminalIconButton>
            </>
          ) : null}
          <TerminalIconButton
            label={expanded ? "Collapse terminal" : "Expand terminal"}
            onClick={handleExpandToggle}
          >
            {expanded ? (
              <PanelBottomClose className="h-3.5 w-3.5" />
            ) : (
              <PanelBottomOpen className="h-3.5 w-3.5" />
            )}
          </TerminalIconButton>
          {expanded ? (
            <TerminalIconButton
              label="Close terminal"
              onClick={handleClose}
              disabled={terminalSessionUnavailable}
            >
              <X className="h-3.5 w-3.5" />
            </TerminalIconButton>
          ) : null}
        </div>
      </div>

      {shouldRenderTerminalBody && (
        <div
          className={cn(
            "relative w-full overflow-hidden",
            expanded ? "h-[calc(100%-2.25rem)]" : "h-0",
          )}
          aria-hidden={!expanded}
        >
          {isHydrating && expanded && (
            <div
              className="absolute inset-0 flex items-start px-3 py-2 font-mono text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              Starting terminal...
            </div>
          )}
          <div
            ref={containerRef}
            className="h-full w-full px-3 py-2"
            aria-label={`Terminal for ${branchLabel}`}
          />
        </div>
      )}
    </div>,
    portalRoot,
  );
}

const TERMINAL_PLACEMENT_OPTIONS: Array<{
  value: AgentTerminalPlacement;
  label: string;
}> = [
  { value: "auto", label: "Auto" },
  { value: "chat", label: "Under chat" },
  { value: "panel", label: "Side panel" },
];

const TERMINAL_PLACEMENT_LABELS: Record<AgentTerminalPlacement, string> =
  Object.fromEntries(
    TERMINAL_PLACEMENT_OPTIONS.map((option) => [option.value, option.label]),
  ) as Record<AgentTerminalPlacement, string>;

function isAgentTerminalPlacement(value: string): value is AgentTerminalPlacement {
  return value === "auto" || value === "chat" || value === "panel";
}

function TerminalPlacementButton({
  placement,
  onPlacementChange,
}: {
  placement: AgentTerminalPlacement;
  onPlacementChange: (placement: AgentTerminalPlacement) => void;
}) {
  const handleValueChange = (value: string) => {
    if (isAgentTerminalPlacement(value) && value !== placement) {
      onPlacementChange(value);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 gap-1 px-2 text-[0.625rem]"
          aria-label={`Terminal dock: ${TERMINAL_PLACEMENT_LABELS[placement]}`}
          data-testid="agent-terminal-placement"
        >
          {TERMINAL_PLACEMENT_LABELS[placement]}
          <ChevronDown className="h-3 w-3" aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="top" className="min-w-[150px]">
        <DropdownMenuRadioGroup value={placement} onValueChange={handleValueChange}>
          {TERMINAL_PLACEMENT_OPTIONS.map((option) => (
            <DropdownMenuRadioItem
              key={option.value}
              value={option.value}
              className="text-xs"
            >
              {option.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function TerminalIconButton({
  label,
  onClick,
  disabled = false,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 w-7 p-0"
          onClick={onClick}
          disabled={disabled}
          aria-label={label}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top" className="text-xs">
        {label}
      </TooltipContent>
    </Tooltip>
  );
}

function readTerminalTheme(): ITheme {
  if (typeof window === "undefined") {
    return {};
  }

  const style = window.getComputedStyle(document.documentElement);
  const read = (name: string) => style.getPropertyValue(name).trim();
  const theme: Record<string, string> = {};
  const set = (key: keyof ITheme, value: string) => {
    if (value) {
      theme[key] = value;
    }
  };

  set("background", read("--bg-base"));
  set("foreground", read("--text-primary"));
  set("cursor", read("--accent-primary"));
  set("cursorAccent", read("--bg-base"));
  set("selectionBackground", read("--overlay-weak"));
  set("black", read("--text-muted"));
  set("brightBlack", read("--text-secondary"));
  set("red", read("--danger"));
  set("green", read("--success"));
  set("yellow", read("--warning"));
  set("blue", read("--accent-primary"));
  set("magenta", read("--accent-secondary"));
  set("cyan", read("--info"));
  set("white", read("--text-primary"));

  return theme as ITheme;
}
