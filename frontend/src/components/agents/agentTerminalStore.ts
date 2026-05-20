import { create } from "zustand";
import { persist } from "zustand/middleware";
import { immer } from "zustand/middleware/immer";

export type AgentTerminalPlacement = "auto" | "chat" | "panel";
export type AgentTerminalDock = "chat" | "panel";

interface AgentTerminalUiState {
  openByConversationId: Record<string, boolean>;
  heightByConversationId: Record<string, number>;
  activeTerminalByConversationId: Record<string, string>;
  placement: AgentTerminalPlacement;
  draggingConversationId: string | null;
  dragOverDock: AgentTerminalDock | null;
}

interface AgentTerminalUiActions {
  setOpen: (conversationId: string, open: boolean) => void;
  toggleOpen: (conversationId: string) => void;
  setHeight: (conversationId: string, height: number) => void;
  setActiveTerminal: (conversationId: string, terminalId: string) => void;
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
      partialize: (state) => ({
        openByConversationId: state.openByConversationId,
        heightByConversationId: state.heightByConversationId,
        activeTerminalByConversationId: state.activeTerminalByConversationId,
        placement: state.placement,
      }),
    }
  )
);
