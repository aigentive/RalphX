function nonEmpty(value) {
    const trimmed = value?.trim();
    return trimmed && trimmed.length > 0 ? trimmed : undefined;
}
export function resolveAgentTaskContext(runtimeContext) {
    const parentConversationId = nonEmpty(runtimeContext.parentConversationId);
    const projectId = nonEmpty(runtimeContext.projectId);
    const actorAgent = nonEmpty(runtimeContext.actorAgent);
    const contextType = nonEmpty(runtimeContext.contextType);
    const contextId = nonEmpty(runtimeContext.contextId);
    if (contextType === "delegation" && contextId) {
        return {
            context_type: contextType,
            context_id: contextId,
            ...(projectId ? { project_id: projectId } : {}),
            ...(actorAgent ? { actor_agent: actorAgent } : {}),
        };
    }
    if (parentConversationId) {
        return {
            context_type: "conversation",
            context_id: parentConversationId,
            ...(projectId ? { project_id: projectId } : {}),
            ...(actorAgent ? { actor_agent: actorAgent } : {}),
        };
    }
    if (contextType && contextId) {
        return {
            context_type: contextType,
            context_id: contextId,
            ...(projectId ? { project_id: projectId } : {}),
            ...(actorAgent ? { actor_agent: actorAgent } : {}),
        };
    }
    return {
        ...(projectId ? { project_id: projectId } : {}),
        ...(actorAgent ? { actor_agent: actorAgent } : {}),
    };
}
export function withAgentTaskRuntimeContext(args, runtimeContext) {
    return {
        ...args,
        ...resolveAgentTaskContext(runtimeContext),
    };
}
//# sourceMappingURL=agent-task-context.js.map