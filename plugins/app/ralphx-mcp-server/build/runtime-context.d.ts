type RuntimeContextKey = "agentType" | "taskId" | "projectId" | "workingDirectory" | "contextType" | "contextId" | "leadSessionId" | "tauriApiUrl";
type RuntimeContext = Partial<Record<RuntimeContextKey, string>>;
export declare function parseCliOptionFromArgs(args: readonly string[], optionName: string): string | undefined;
export declare function hydrateRalphxRuntimeEnvFromCli(args: readonly string[], env?: NodeJS.ProcessEnv): RuntimeContext;
export {};
//# sourceMappingURL=runtime-context.d.ts.map