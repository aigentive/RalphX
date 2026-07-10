import { z } from "zod";

export const NotificationCategorySchema = z.enum([
  "review_needed", "review_escalated", "qa_failed", "merge_conflict",
  "merge_incomplete", "task_failed", "task_blocked", "task_stuck",
  "provider_paused", "recovery_prompt", "permission_request", "agent_question",
  "plan_approval", "team_plan_approval", "automation_plan_approval",
  "automation_paused", "automation_run_failed", "automation_run_completed",
  "agent_waiting", "gh_auth", "git_auth_preflight", "pr_review_action", "info",
]);

export type NotificationCategory = z.infer<typeof NotificationCategorySchema>;

/** New backend categories degrade to a neutral row until this UI gains a mapping. */
export const AttentionCategorySchema = z.string().transform((value): NotificationCategory =>
  NotificationCategorySchema.safeParse(value).data ?? "info",
);

export const NotificationTargetSchema = z.object({
  kind: z.enum(["task", "agent_conversation", "automation_run", "project", "none"]),
  projectId: z.string().optional(),
  taskId: z.string().optional(),
  conversationId: z.string().optional(),
  setupConversationId: z.string().optional(),
  automationId: z.string().optional(),
  runId: z.string().optional(),
});

export const AttentionItemSchema = z.object({
  id: z.string(),
  category: AttentionCategorySchema,
  title: z.string(),
  detail: z.string().optional(),
  projectId: z.string().optional(),
  createdAt: z.string().optional(),
  target: NotificationTargetSchema,
});

export const AttentionItemListSchema = z.array(AttentionItemSchema);
export type AttentionItem = z.infer<typeof AttentionItemSchema>;
export type NotificationTarget = z.infer<typeof NotificationTargetSchema>;
