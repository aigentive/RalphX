import { create } from "zustand";

import type { ComposerSelectionSnapshot } from "@/api/chat";

interface ArtifactSelectionState {
  selections: Record<string, ComposerSelectionSnapshot | undefined>;
  setSelection: (
    conversationId: string,
    snapshot: ComposerSelectionSnapshot,
  ) => void;
  clearSelection: (conversationId: string) => void;
  clearAllSelections: () => void;
}

export const useArtifactSelectionStore = create<ArtifactSelectionState>(
  (set) => ({
    selections: {},
    setSelection: (conversationId, snapshot) =>
      set((state) => ({
        selections: {
          ...state.selections,
          [conversationId]: snapshot,
        },
      })),
    clearSelection: (conversationId) =>
      set((state) => {
        const selections = { ...state.selections };
        delete selections[conversationId];
        return { selections };
      }),
    clearAllSelections: () => set({ selections: {} }),
  }),
);

export const selectArtifactSelection = (conversationId: string | null) =>
  (state: ArtifactSelectionState): ComposerSelectionSnapshot | null =>
    conversationId ? (state.selections[conversationId] ?? null) : null;
