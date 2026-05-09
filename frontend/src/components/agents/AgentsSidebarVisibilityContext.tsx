import { createContext } from "react";

export interface AgentsSidebarVisibility {
  isCollapsed: boolean;
  onToggle: () => void;
}

export const AgentsSidebarVisibilityContext =
  createContext<AgentsSidebarVisibility | null>(null);
