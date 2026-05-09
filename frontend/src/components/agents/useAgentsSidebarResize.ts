import type { RefObject } from "react";

import { usePersistentSidebarResize } from "@/hooks/usePersistentSidebarResize";

const AGENTS_SIDEBAR_WIDTH_STORAGE_KEY = "ralphx-agents-sidebar-width";

export const AGENTS_SIDEBAR_MIN_WIDTH = 220;
export const AGENTS_SIDEBAR_MAX_WIDTH = 520;

export function useAgentsSidebarResize(
  sidebarRef: RefObject<HTMLDivElement | null>,
) {
  return usePersistentSidebarResize(sidebarRef, {
    maxWidth: AGENTS_SIDEBAR_MAX_WIDTH,
    minWidth: AGENTS_SIDEBAR_MIN_WIDTH,
    storageKey: AGENTS_SIDEBAR_WIDTH_STORAGE_KEY,
  });
}
