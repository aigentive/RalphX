import type { ReactNode } from "react";

import {
  AgentsSidebarVisibilityContext,
  type AgentsSidebarVisibility,
} from "./AgentsSidebarVisibilityContext";

export function AgentsSidebarVisibilityProvider({
  value,
  children,
}: {
  value: AgentsSidebarVisibility;
  children: ReactNode;
}) {
  return (
    <AgentsSidebarVisibilityContext.Provider value={value}>
      {children}
    </AgentsSidebarVisibilityContext.Provider>
  );
}
