import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Terminal as TerminalIcon } from "lucide-react";

import type { AgentConversationWorkspace } from "@/api/chat";
import type { AgentArtifactTab } from "@/stores/agentSessionStore";
import { formatBranchDisplay } from "@/lib/branch-utils";

import { useResolvedAgentArtifactState } from "./agentArtifactState";
import { useAfterPaintMounted } from "./agentDeferredFrame";
import { compactTerminalPath } from "./agentTerminalPaths";
import { preloadAgentTerminalDrawer } from "./agentTerminalPreload";
import {
  AGENT_TERMINAL_COLLAPSED_HEIGHT,
  AGENT_TERMINAL_DEFAULT_HEIGHT,
  useAgentTerminalStore,
  type AgentTerminalPlacement,
} from "./agentTerminalStore";

const LazyAgentTerminalDrawer = lazy(() =>
  preloadAgentTerminalDrawer().then((module) => ({ default: module.AgentTerminalDrawer })),
);

function AgentTerminalLoadingShell({
  height,
  expanded,
  workspace,
  dockElement,
}: {
  height: number;
  expanded: boolean;
  workspace: AgentConversationWorkspace;
  dockElement: HTMLElement | null;
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
        boxShadow: "0 -16px 36px var(--shadow-card)",
      }}
      data-testid="agent-terminal-loading-shell"
    >
      <div
        className="flex h-9 items-center gap-2 border-b px-3 text-xs"
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
        <span>{expanded ? "Opening" : "Closed"}</span>
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

interface AgentsTerminalPresentationInput {
  conversationId: string | null;
  workspace: AgentConversationWorkspace | null;
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
  terminalUnavailableReason,
  hasAutoOpenArtifacts,
  setDockElement,
}: AgentsTerminalDockHostProps) {
  const { canRender, isExpanded, height, dockTarget } = useAgentTerminalPresentation({
    conversationId,
    workspace,
    terminalUnavailableReason,
    hasAutoOpenArtifacts,
  });

  if (!canRender) {
    return null;
  }

  const isVisible = dockTarget === dock;

  return (
    <div
      ref={setDockElement}
      className="shrink-0 overflow-hidden"
      style={{
        height: isVisible
          ? isExpanded
            ? height
            : AGENT_TERMINAL_COLLAPSED_HEIGHT
          : 0,
        opacity: isVisible ? 1 : 0,
        pointerEvents: isVisible ? "auto" : "none",
        transition: "none",
      }}
      data-testid={dock === "panel" ? "agent-terminal-host-panel" : "agent-terminal-host-chat"}
    />
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
    terminalUnavailableReason,
    hasAutoOpenArtifacts,
  });
  const [hasActivatedDrawer, setHasActivatedDrawer] = useState(false);
  useEffect(() => {
    if (!canRender) {
      setHasActivatedDrawer(false);
      return;
    }
    if (isExpanded) {
      setHasActivatedDrawer(true);
    }
  }, [canRender, isExpanded]);
  const contentMounted = useAfterPaintMounted(canRender && hasActivatedDrawer);
  const setTerminalHeight = useAgentTerminalStore((state) => state.setHeight);
  const setTerminalOpen = useAgentTerminalStore((state) => state.setOpen);
  const setTerminalPlacement = useAgentTerminalStore((state) => state.setPlacement);

  const handlePlacementChange = useCallback(
    (nextPlacement: AgentTerminalPlacement) => {
      setTerminalPlacement(nextPlacement);
      if (nextPlacement === "panel" && conversationId && !artifactPaneOpen) {
        onOpenArtifactTab(conversationId, "publish");
      }
    },
    [artifactPaneOpen, conversationId, onOpenArtifactTab, setTerminalPlacement],
  );

  if (!canRender || !conversationId || !workspace) {
    return null;
  }

  const dockElement = dockTarget === "panel" ? panelDockElement : chatDockElement;

  if (!contentMounted) {
    return (
      <AgentTerminalLoadingShell
        height={height}
        expanded={isExpanded}
        workspace={workspace}
        dockElement={dockElement}
      />
    );
  }

  return (
    <Suspense
      fallback={
        <AgentTerminalLoadingShell
          height={height}
          expanded={isExpanded}
          workspace={workspace}
          dockElement={dockElement}
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
        dockElement={dockElement}
      />
    </Suspense>
  );
}
