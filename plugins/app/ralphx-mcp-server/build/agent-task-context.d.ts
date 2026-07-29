export interface AgentTaskRuntimeContext {
    contextType?: string;
    contextId?: string;
    projectId?: string;
    actorAgent?: string;
    conversationId?: string;
    parentConversationId?: string;
    agentRunId?: string;
}
export declare function resolveAgentTaskContext(runtimeContext: AgentTaskRuntimeContext): Record<string, string>;
export declare function withAgentTaskRuntimeContext(args: Record<string, unknown>, runtimeContext: AgentTaskRuntimeContext): Record<string, unknown>;
//# sourceMappingURL=agent-task-context.d.ts.map