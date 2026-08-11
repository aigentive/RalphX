import type { LucideIcon } from "lucide-react";
import {
  Bot, FileCheck, FlaskConical, GitMerge, GitPullRequest, GitPullRequestArrow,
  Hand, Inbox, KeyRound, LifeBuoy, MessageCircleQuestion, PauseCircle,
  ShieldQuestion, TriangleAlert, XCircle,
} from "lucide-react";

import type { NotificationCategory } from "@/types/notifications";

export type AttentionGroup = "Agent requests" | "Reviews" | "Tasks" | "Automations" | "Git";
export interface AttentionCategoryPresentation {
  action: string | null;
  group: AttentionGroup;
  icon: LucideIcon;
  iconColor: string;
}

/** Shared category presentation contract for panel, toasts, and future history rows. */
export const ATTENTION_CATEGORY_MAPPING: Record<NotificationCategory, AttentionCategoryPresentation> = {
  permission_request: { action: "Respond", group: "Agent requests", icon: ShieldQuestion, iconColor: "var(--status-warning)" },
  agent_question: { action: "Answer", group: "Agent requests", icon: MessageCircleQuestion, iconColor: "var(--accent-primary)" },
  plan_approval: { action: "Review plan", group: "Agent requests", icon: FileCheck, iconColor: "var(--accent-primary)" },
  review_needed: { action: "Review", group: "Reviews", icon: GitPullRequest, iconColor: "var(--accent-primary)" },
  review_escalated: { action: "Decide", group: "Reviews", icon: TriangleAlert, iconColor: "var(--status-warning)" },
  qa_failed: { action: "Open task", group: "Tasks", icon: FlaskConical, iconColor: "var(--status-error)" },
  merge_conflict: { action: "Resolve", group: "Tasks", icon: GitMerge, iconColor: "var(--status-error)" },
  merge_incomplete: { action: "Open task", group: "Tasks", icon: GitMerge, iconColor: "var(--status-warning)" },
  task_failed: { action: "Open task", group: "Tasks", icon: XCircle, iconColor: "var(--status-error)" },
  task_blocked: { action: "Open task", group: "Tasks", icon: Hand, iconColor: "var(--status-warning)" },
  task_stuck: { action: "Open task", group: "Tasks", icon: LifeBuoy, iconColor: "var(--status-error)" },
  provider_paused: { action: null, group: "Tasks", icon: PauseCircle, iconColor: "var(--status-warning)" },
  recovery_prompt: { action: "Open task", group: "Tasks", icon: LifeBuoy, iconColor: "var(--status-error)" },
  automation_plan_approval: { action: "Review plan", group: "Automations", icon: Bot, iconColor: "var(--accent-primary)" },
  automation_paused: { action: "Resume", group: "Automations", icon: Bot, iconColor: "var(--status-warning)" },
  automation_run_failed: { action: "Open run", group: "Automations", icon: Bot, iconColor: "var(--status-error)" },
  automation_run_completed: { action: "Open run", group: "Automations", icon: Bot, iconColor: "var(--status-success)" },
  gh_auth: { action: "Enter code", group: "Git", icon: KeyRound, iconColor: "var(--status-warning)" },
  git_auth_preflight: { action: "Open settings", group: "Git", icon: KeyRound, iconColor: "var(--status-warning)" },
  pr_review_action: { action: "Review action", group: "Git", icon: GitPullRequestArrow, iconColor: "var(--accent-primary)" },
  agent_waiting: { action: null, group: "Agent requests", icon: Inbox, iconColor: "var(--text-muted)" },
  info: { action: null, group: "Git", icon: Inbox, iconColor: "var(--text-muted)" },
};
