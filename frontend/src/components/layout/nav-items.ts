/**
 * Shared nav item config used by the top-bar Navigation and the left-rail nav.
 * Order matches main navigation shortcut map: ⌘1 through ⌘5.
 */

import {
  Activity,
  Briefcase,
  GitBranch,
  LayoutGrid,
  Lightbulb,
  Puzzle,
  Ticket,
  TrendingUp,
  Workflow,
} from "lucide-react";
import { GitHubMarkIcon } from "@/components/github/GitHubMarkIcon";
import { GranolaIcon } from "@/components/granola/GranolaIcon";
import type { FeatureFlags } from "@/types/feature-flags";
import type { ViewType } from "@/types/chat";

export interface NavItemConfig {
  view: ViewType;
  label: string;
  icon: React.ElementType;
  shortcut?: string;
  visible: (flags: FeatureFlags) => boolean;
}

export const ALL_NAV_ITEMS: NavItemConfig[] = [
  {
    view: "agents",
    label: "Agents",
    icon: Briefcase,
    shortcut: "⌘1",
    visible: () => true,
  },
  {
    view: "ideation",
    label: "Ideation",
    icon: Lightbulb,
    shortcut: "⌘2",
    visible: (flags) => flags.ideationPage,
  },
  {
    view: "graph",
    label: "Graph",
    icon: GitBranch,
    shortcut: "⌘3",
    visible: () => true,
  },
  {
    view: "kanban",
    label: "Kanban",
    icon: LayoutGrid,
    shortcut: "⌘4",
    visible: () => true,
  },
  {
    view: "automations",
    label: "Automations",
    icon: Workflow,
    visible: (flags) => flags.automationsPage,
  },
  {
    view: "ticketing",
    label: "Ticketing",
    icon: Ticket,
    visible: () => true,
  },
  {
    view: "github",
    label: "GitHub",
    icon: GitHubMarkIcon,
    visible: () => true,
  },
  {
    view: "granola",
    label: "Granola",
    icon: GranolaIcon,
    visible: () => true,
  },
  {
    view: "insights",
    label: "Insights",
    icon: TrendingUp,
    shortcut: "⌘5",
    visible: () => true,
  },
  {
    view: "extensibility",
    label: "Extensibility",
    icon: Puzzle,
    visible: (flags) => flags.extensibilityPage,
  },
  {
    view: "activity",
    label: "Activity",
    icon: Activity,
    visible: (flags) => flags.activityPage,
  },
];
