import { z } from "zod";

const OptionalStringSchema = z.string().nullish().transform((value) => value ?? undefined);

export const NotificationCategorySchema = z.enum([
  "review_needed", "review_escalated", "qa_failed", "merge_conflict",
  "merge_incomplete", "task_failed", "task_blocked", "task_stuck",
  "provider_paused", "recovery_prompt", "permission_request", "agent_question",
  "plan_approval", "automation_plan_approval",
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
  detail: OptionalStringSchema,
  projectId: OptionalStringSchema,
  createdAt: OptionalStringSchema,
  target: NotificationTargetSchema,
});

export const AttentionItemListSchema = z.array(AttentionItemSchema);

const NotificationSeverityValueSchema = z.enum(["action_required", "warning", "info"]);

/** New backend severities degrade to a neutral row until this UI gains a mapping. */
export const NotificationSeveritySchema = z.string().transform((value): z.infer<typeof NotificationSeverityValueSchema> =>
  NotificationSeverityValueSchema.safeParse(value).data ?? "info",
);

/** Durable history row emitted by the notification service. */
export const NotificationSchema = z.object({
  id: z.string(),
  createdAt: z.string(),
  projectId: OptionalStringSchema,
  category: AttentionCategorySchema,
  severity: NotificationSeveritySchema,
  title: z.string(),
  body: OptionalStringSchema,
  target: NotificationTargetSchema,
  dedupeKey: OptionalStringSchema,
  readAt: OptionalStringSchema,
});

export const NotificationPageSchema = z.object({
  notifications: z.array(NotificationSchema),
  cursor: OptionalStringSchema,
  hasMore: z.boolean(),
});

export type AttentionItem = z.infer<typeof AttentionItemSchema>;
export type NotificationTarget = z.infer<typeof NotificationTargetSchema>;
export type Notification = z.infer<typeof NotificationSchema>;
export type NotificationPage = z.infer<typeof NotificationPageSchema>;
