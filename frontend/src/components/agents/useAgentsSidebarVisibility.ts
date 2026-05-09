import { useContext } from "react";

import {
  AgentsSidebarVisibilityContext,
  type AgentsSidebarVisibility,
} from "./AgentsSidebarVisibilityContext";

export function useAgentsSidebarVisibility(): AgentsSidebarVisibility | null {
  return useContext(AgentsSidebarVisibilityContext);
}
