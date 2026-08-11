export interface AgentTaskRuntimeContext {
  contextType?: string;
  contextId?: string;
  projectId?: string;
  actorAgent?: string;
  conversationId?: string;
  parentConversationId?: string;
  agentRunId?: string;
}

function nonEmpty(value: string | undefined): string | undefined {
  const trimmed = value?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : undefined;
}

export function resolveAgentTaskContext(
  runtimeContext: AgentTaskRuntimeContext
): Record<string, string> {
  const parentConversationId = nonEmpty(runtimeContext.parentConversationId);
  const projectId = nonEmpty(runtimeContext.projectId);
  const actorAgent = nonEmpty(runtimeContext.actorAgent);
  const conversationId = nonEmpty(runtimeContext.conversationId);
  const contextType = nonEmpty(runtimeContext.contextType);
  const contextId = nonEmpty(runtimeContext.contextId);
  const agentRunId = nonEmpty(runtimeContext.agentRunId);

  if (contextType === "delegation" && contextId) {
    return {
      context_type: contextType,
      context_id: contextId,
      ...(projectId ? { project_id: projectId } : {}),
      ...(actorAgent ? { actor_agent: actorAgent } : {}),
    };
  }

  const ledgerConversationId = parentConversationId ?? conversationId;
  if (ledgerConversationId) {
    return {
      context_type: "conversation",
      context_id: ledgerConversationId,
      ...(projectId ? { project_id: projectId } : {}),
      ...(actorAgent ? { actor_agent: actorAgent } : {}),
    };
  }

  if (contextType && contextId && contextType !== "project") {
    return {
      context_type: contextType,
      context_id: contextId,
      ...(projectId ? { project_id: projectId } : {}),
      ...(actorAgent ? { actor_agent: actorAgent } : {}),
    };
  }

  if (agentRunId) {
    return {
      context_type: "agent_run",
      context_id: agentRunId,
      ...(projectId ? { project_id: projectId } : {}),
      ...(actorAgent ? { actor_agent: actorAgent } : {}),
    };
  }

  if (contextType === "project" || projectId) {
    throw new Error(
      "Agent task ledger requires conversation identity; refusing shared project scope."
    );
  }

  return {
    ...(actorAgent ? { actor_agent: actorAgent } : {}),
  };
}

export function withAgentTaskRuntimeContext(
  args: Record<string, unknown>,
  runtimeContext: AgentTaskRuntimeContext
): Record<string, unknown> {
  return {
    ...args,
    ...resolveAgentTaskContext(runtimeContext),
  };
}
