import { createContext, useContext } from "react";

import type { BulkArchiveConversationTarget } from "./bulkConversationArchive";

export interface BulkArchiveSelectionContextValue {
  active: boolean;
  pending: boolean;
  selectedIds: ReadonlySet<string>;
  toggleTarget: (target: BulkArchiveConversationTarget) => void;
  registerLoadedTargets: (
    sourceKey: string,
    targets: BulkArchiveConversationTarget[]
  ) => void;
  unregisterLoadedTargets: (sourceKey: string) => void;
}

export const BulkArchiveSelectionContext =
  createContext<BulkArchiveSelectionContextValue | null>(null);

export function useBulkArchiveSelection() {
  const value = useContext(BulkArchiveSelectionContext);
  if (!value) {
    throw new Error("BulkArchiveSelectionContext.Provider is missing");
  }
  return value;
}
