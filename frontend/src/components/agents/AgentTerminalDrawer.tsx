import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import type { FitAddon } from "@xterm/addon-fit";
import type {
  Terminal as XTermTerminal,
  IDisposable,
  ITheme,
} from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  PanelBottomClose,
  RefreshCw,
  Terminal as TerminalIcon,
  Trash2,
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
  type AgentTerminalStatus,
} from "@/api/terminal";
import type { AgentConversationWorkspace } from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatBranchDisplay } from "@/lib/branch-utils";
import { cn } from "@/lib/utils";
import { compactTerminalPath } from "./agentTerminalPaths";
import { loadAgentTerminalRuntime } from "./agentTerminalRuntime";
import type { AgentTerminalPlacement } from "./agentTerminalStore";

interface AgentTerminalDrawerProps {
  conversationId: string;
  workspace: AgentConversationWorkspace;
  height: number;
  onHeightChange: (height: number) => void;
  onClose: () => void;
  placement: AgentTerminalPlacement;
  onPlacementChange: (placement: AgentTerminalPlacement) => void;
  dockElement: HTMLElement | null;
}

const TERMINAL_MIN_COLS = 80;
const TERMINAL_MIN_ROWS = 20;

type DeferredFrameJob = { frame: number | null; timer: number | null };

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
  onHeightChange,
  onClose,
  placement,
  onPlacementChange,
  dockElement,
}: AgentTerminalDrawerProps) {
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
  const [status, setStatus] = useState<AgentTerminalStatus>("running");
  const [cwd, setCwd] = useState(workspace.worktreePath);
  const [branchName, setBranchName] = useState(workspace.branchName);
  const [isFocused, setIsFocused] = useState(false);
  const [isRestarting, setIsRestarting] = useState(false);
  const [isClearing, setIsClearing] = useState(false);
  const [isHydrating, setIsHydrating] = useState(true);

  const branchLabel = useMemo(
    () => formatBranchDisplay(branchName).short,
    [branchName],
  );
  const displayCwd = useMemo(() => compactTerminalPath(cwd), [cwd]);

  const terminalTheme = useMemo(() => readTerminalTheme(), []);

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

  const applySnapshot = useCallback((snapshot: AgentTerminalSnapshot) => {
    setStatus(snapshot.status);
    setCwd(snapshot.cwd);
    setBranchName(snapshot.workspaceBranch);
  }, []);

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
      setStatus("running");
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
      setStatus("exited");
      terminal?.write("\r\n[terminal exited]\r\n");
      return;
    }

    if (event.type === "error") {
      setStatus("error");
      if (event.message) {
        terminal?.write(`\r\n[terminal error] ${event.message}\r\n`);
      }
    }
  }, [conversationId, terminalId]);

  const showControlError = useCallback((error: unknown) => {
    const message = error instanceof Error ? error.message : "Terminal command failed";
    setStatus("error");
    terminalRef.current?.write(`\r\n[terminal error] ${message}\r\n`);
  }, []);

  useEffect(() => {
    if (!hasDocked) {
      return;
    }
    const host = containerRef.current;
    if (!host) {
      return;
    }

    hydrationCompleteRef.current = false;
    bufferedEventsRef.current = [];
    setIsHydrating(true);

    let disposed = false;
    let terminal: XTermTerminal | null = null;
    let dataDisposable: IDisposable | null = null;
    let resizeFrame: number | null = null;
    let initFrame: number | null = null;
    let initTimer: number | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let unlisten: (() => void) | null = null;
    let listenerPromise: Promise<void> | null = null;

    const releaseListener = () => {
      if (unlisten) {
        unlisten();
        unlisten = null;
      }
    };

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

      listenerPromise = listen<unknown>(AGENT_TERMINAL_EVENT, (event) => {
        const parsed = AgentTerminalEventSchema.safeParse(event.payload);
        if (parsed.success) {
          applyEvent(parsed.data);
        }
      }).then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }
        unlisten = dispose;
      });
      await listenerPromise;

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
          setStatus("error");
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
      releaseListener();
      void listenerPromise?.then(releaseListener);
      terminal?.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [
    applyEvent,
    applySnapshot,
    conversationId,
    fitTerminal,
    fitAndReportSize,
    hasDocked,
    showControlError,
    terminalId,
    terminalTheme,
  ]);

  const hasTerminalInstance = useCallback(() => {
    if (!terminalRef.current) {
      setStatus("error");
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
  }, [applySnapshot, conversationId, showControlError, terminalId]);

  const handleClose = useCallback(() => {
    onClose();
    void closeAgentTerminal({ conversationId, terminalId }).catch(() => undefined);
  }, [conversationId, onClose, terminalId]);

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

  return createPortal(
    <div
      className={cn(
        "relative shrink-0 overflow-hidden border-t",
        isFocused && "border-t-2",
      )}
      style={{
        height,
        background: "var(--bg-base)",
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
          background: "var(--bg-surface)",
          borderColor: "var(--overlay-faint)",
        }}
      >
        <div className="flex min-w-0 items-center gap-2 text-xs">
          <TerminalIcon
            className="h-3.5 w-3.5 shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
          <span className="font-medium" style={{ color: "var(--text-primary)" }}>
            Terminal
          </span>
          <span className="h-1 w-1 rounded-full" style={{ background: "var(--text-muted)" }} />
          <span className="shrink-0 capitalize" style={{ color: "var(--text-secondary)" }}>
            {isHydrating ? "Opening" : status}
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
          <TerminalIconButton
            label="Clear terminal"
            onClick={() => void handleClear()}
            disabled={isClearing || isHydrating}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </TerminalIconButton>
          <TerminalIconButton
            label="Start fresh terminal session"
            onClick={() => void handleRestart()}
            disabled={isRestarting || isHydrating}
          >
            <RefreshCw className={cn("h-3.5 w-3.5", isRestarting && "animate-spin")} />
          </TerminalIconButton>
          <TerminalIconButton label="Close terminal" onClick={handleClose}>
            <PanelBottomClose className="h-3.5 w-3.5" />
          </TerminalIconButton>
        </div>
      </div>

      <div className="relative h-[calc(100%-2.25rem)] w-full">
        {isHydrating && (
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
    </div>,
    portalRoot,
  );
}

const TERMINAL_PLACEMENT_LABELS: Record<AgentTerminalPlacement, string> = {
  auto: "Auto",
  chat: "Chat",
  panel: "Panel",
};

function TerminalPlacementButton({
  placement,
  onPlacementChange,
}: {
  placement: AgentTerminalPlacement;
  onPlacementChange: (placement: AgentTerminalPlacement) => void;
}) {
  const nextPlacement: AgentTerminalPlacement =
    placement === "auto" ? "panel" : placement === "panel" ? "chat" : "auto";
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-[0.625rem]"
          onClick={() => onPlacementChange(nextPlacement)}
          aria-label={`Terminal dock: ${TERMINAL_PLACEMENT_LABELS[placement]}`}
          data-testid="agent-terminal-placement"
        >
          {TERMINAL_PLACEMENT_LABELS[placement]}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="top" className="text-xs">
        Move terminal docking
      </TooltipContent>
    </Tooltip>
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
