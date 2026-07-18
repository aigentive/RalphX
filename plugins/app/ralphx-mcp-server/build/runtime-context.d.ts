type RuntimeContextKey = "agentType" | "agentProfile" | "taskId" | "taskState" | "projectId" | "workingDirectory" | "filesystemReadRoots" | "contextType" | "contextId" | "conversationId" | "coordinationMode" | "parentConversationId" | "agentRunId" | "leadSessionId" | "tauriApiUrl" | "traceDir";
export type RuntimeContext = Partial<Record<RuntimeContextKey, string>>;
export declare function parseCliOptionFromArgs(args: readonly string[], optionName: string): string | undefined;
export declare function parseCliOptionsFromArgs(args: readonly string[], optionName: string): string[];
export declare function hydrateRalphxRuntimeEnvFromCli(args: readonly string[], env?: NodeJS.ProcessEnv): RuntimeContext;
export declare function buildArtifactMutationTransportHeaders(context: RuntimeContext): Record<string, string> | undefined;
export declare function buildRuntimeTransportHeaders(context: RuntimeContext): Record<string, string> | undefined;
export {};
//# sourceMappingURL=runtime-context.d.ts.map