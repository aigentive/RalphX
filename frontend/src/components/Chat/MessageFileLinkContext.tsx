import { createContext, useContext } from "react";

import type { WorkspaceOpenTarget } from "@/api/chat";

export interface MessageFileLinkContextValue {
  workspaceRootPath: string;
  targets: readonly WorkspaceOpenTarget[];
  preferredTargetId: string | null;
  openingPath: string | null;
  openingTargetId: string | null;
  onOpenPath: (path: string, targetId: string) => void;
}

export const MessageFileLinkContext =
  createContext<MessageFileLinkContextValue | null>(null);

export function useMessageFileLinkContext(): MessageFileLinkContextValue | null {
  return useContext(MessageFileLinkContext);
}
