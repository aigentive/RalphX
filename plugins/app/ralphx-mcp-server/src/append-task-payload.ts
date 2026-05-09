export type AppendTaskToIdeationPlanToolArgs = {
  project_id?: unknown;
  session_id?: unknown;
  title?: unknown;
  description?: unknown;
  steps?: unknown;
  acceptance_criteria?: unknown;
  depends_on_task_ids?: unknown;
  priority?: unknown;
  source_conversation_id?: unknown;
  source_message_id?: unknown;
};

export function buildAppendTaskToIdeationPlanPayload(
  args: AppendTaskToIdeationPlanToolArgs
): Record<string, unknown> {
  const payload: Record<string, unknown> = {};

  if (args.project_id !== undefined) payload.projectId = args.project_id;
  if (args.session_id !== undefined) payload.sessionId = args.session_id;
  if (args.title !== undefined) payload.title = args.title;
  if (args.description !== undefined) payload.description = args.description;
  if (args.steps !== undefined) payload.steps = args.steps;
  if (args.acceptance_criteria !== undefined) {
    payload.acceptanceCriteria = args.acceptance_criteria;
  }
  if (args.depends_on_task_ids !== undefined) {
    payload.dependsOnTaskIds = args.depends_on_task_ids;
  }
  if (args.priority !== undefined) payload.priority = args.priority;
  if (args.source_conversation_id !== undefined) {
    payload.sourceConversationId = args.source_conversation_id;
  }
  if (args.source_message_id !== undefined) {
    payload.sourceMessageId = args.source_message_id;
  }

  return payload;
}
