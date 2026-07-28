import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

import { createEnvScopedStorage } from "@/lib/remote/env-scoped-storage";
import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
export type AgentTerminalPlacement = "auto" | "chat" | "panel";
export type AgentTerminalDock = "chat" | "panel";
export type AgentTerminalCachedStatus = "closed" | "running" | "exited" | "error";

export interface AgentTerminalConversationMetadata {
  conversationId: string;
  projectId: string;
  title: string | null;
  branchName: string | null;
  worktreePath: string | null;
  updatedAt: string | null;
}

interface AgentTerminalUiState {
  openByConversationId: Record<string, boolean>;
  heightByConversationId: Record<string, number>;
  activeTerminalByConversationId: Record<string, string>;
  statusByConversationId: Record<string, AgentTerminalCachedStatus>;
  metadataByConversationId: Record<string, AgentTerminalConversationMetadata>;
  placement: AgentTerminalPlacement;
  draggingConversationId: string | null;
  dragOverDock: AgentTerminalDock | null;
}

interface AgentTerminalUiActions {
  setOpen: (conversationId: string, open: boolean) => void;
  toggleOpen: (conversationId: string) => void;
  setHeight: (conversationId: string, height: number) => void;
  setActiveTerminal: (conversationId: string, terminalId: string) => void;
  setStatus: (conversationId: string, status: AgentTerminalCachedStatus) => void;
  registerConversation: (metadata: AgentTerminalConversationMetadata) => void;
  setPlacement: (placement: AgentTerminalPlacement) => void;
  setDraggingConversation: (conversationId: string | null) => void;
  setDragOverDock: (dock: AgentTerminalDock | null) => void;
  clearDragState: () => void;
}

export const AGENT_TERMINAL_DEFAULT_HEIGHT = 260;
export const AGENT_TERMINAL_COLLAPSED_HEIGHT = 36;
export const AGENT_TERMINAL_MIN_HEIGHT = 160;
export const AGENT_TERMINAL_MAX_HEIGHT = 560;

export const useAgentTerminalStore = create<
  AgentTerminalUiState & AgentTerminalUiActions
>()(
  persist(
    immer((set) => ({
      openByConversationId: {},
      heightByConversationId: {},
      activeTerminalByConversationId: {},
      statusByConversationId: {},
      metadataByConversationId: {},
      placement: "auto",
      draggingConversationId: null,
      dragOverDock: null,

      setOpen: (conversationId, open) =>
        set((state) => {
          state.openByConversationId[conversationId] = open;
        }),

      toggleOpen: (conversationId) =>
        set((state) => {
          state.openByConversationId[conversationId] =
            !(state.openByConversationId[conversationId] ?? false);
        }),

      setHeight: (conversationId, height) =>
        set((state) => {
          state.heightByConversationId[conversationId] = Math.min(
            AGENT_TERMINAL_MAX_HEIGHT,
            Math.max(AGENT_TERMINAL_MIN_HEIGHT, height)
          );
        }),

      setActiveTerminal: (conversationId, terminalId) =>
        set((state) => {
          state.activeTerminalByConversationId[conversationId] = terminalId;
        }),

      setStatus: (conversationId, status) =>
        set((state) => {
          state.statusByConversationId[conversationId] = status;
        }),

      registerConversation: (metadata) =>
        set((state) => {
          state.metadataByConversationId[metadata.conversationId] = metadata;
        }),

      setPlacement: (placement) =>
        set((state) => {
          state.placement = placement;
        }),

      setDraggingConversation: (conversationId) =>
        set((state) => {
          state.draggingConversationId = conversationId;
          if (!conversationId) {
            state.dragOverDock = null;
          }
        }),

      setDragOverDock: (dock) =>
        set((state) => {
          state.dragOverDock = dock;
        }),

      clearDragState: () =>
        set((state) => {
          state.draggingConversationId = null;
          state.dragOverDock = null;
        }),
    })),
    {
      name: "ralphx-agent-terminal-ui",
      storage: createEnvScopedStorage("ralphx-agent-terminal-ui"),
      partialize: (state) => ({
        openByConversationId: state.openByConversationId,
        heightByConversationId: state.heightByConversationId,
        activeTerminalByConversationId: state.activeTerminalByConversationId,
        metadataByConversationId: state.metadataByConversationId,
        placement: state.placement,
      }),
    }
  )
);

registerEnvIsolatedStore({
  name: "useAgentTerminalStore",
  reset: () => useAgentTerminalStore.setState(useAgentTerminalStore.getInitialState(), true),
  rehydrate: () => {
    void useAgentTerminalStore.persist.rehydrate();
  },
});
