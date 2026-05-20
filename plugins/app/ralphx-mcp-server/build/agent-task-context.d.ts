export interface AgentTaskRuntimeContext {
    contextType?: string;
    contextId?: string;
    projectId?: string;
    actorAgent?: string;
    parentConversationId?: string;
}
export declare function resolveAgentTaskContext(runtimeContext: AgentTaskRuntimeContext): Record<string, string>;
export declare function withAgentTaskRuntimeContext(args: Record<string, unknown>, runtimeContext: AgentTaskRuntimeContext): Record<string, unknown>;
//# sourceMappingURL=agent-task-context.d.ts.map