type RuntimeContextKey = "agentType" | "agentProfile" | "taskId" | "taskState" | "projectId" | "workingDirectory" | "filesystemReadRoots" | "contextType" | "contextId" | "conversationId" | "parentConversationId" | "agentRunId" | "leadSessionId" | "tauriApiUrl" | "traceDir";
type RuntimeContext = Partial<Record<RuntimeContextKey, string>>;
export declare function parseCliOptionFromArgs(args: readonly string[], optionName: string): string | undefined;
export declare function parseCliOptionsFromArgs(args: readonly string[], optionName: string): string[];
export declare function hydrateRalphxRuntimeEnvFromCli(args: readonly string[], env?: NodeJS.ProcessEnv): RuntimeContext;
export {};
//# sourceMappingURL=runtime-context.d.ts.map