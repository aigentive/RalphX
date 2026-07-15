/**
 * Shared nav item config used by the main app navigation surfaces.
 * Numbered shortcuts are derived from this order by the app keyboard handler.
 */

import {
  Activity,
  BookOpenCheck,
  Briefcase,
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
    view: "automations",
    label: "Automations",
    icon: Workflow,
    shortcut: "⌘2",
    visible: (flags) => flags.automationsPage,
  },
  {
    view: "skills",
    label: "Skills",
    icon: BookOpenCheck,
    visible: () => true,
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
    shortcut: "⌘3",
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
