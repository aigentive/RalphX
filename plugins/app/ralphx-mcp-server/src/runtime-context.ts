type RuntimeContextKey =
  | "agentType"
  | "taskId"
  | "projectId"
  | "workingDirectory"
  | "contextType"
  | "contextId"
  | "leadSessionId"
  | "tauriApiUrl";

type RuntimeContext = Partial<Record<RuntimeContextKey, string>>;

const RUNTIME_ARG_ENV_MAPPINGS: Array<{
  key: RuntimeContextKey;
  argName: string;
  envName: string;
}> = [
  { key: "agentType", argName: "agent-type", envName: "RALPHX_AGENT_TYPE" },
  { key: "taskId", argName: "task-id", envName: "RALPHX_TASK_ID" },
  { key: "projectId", argName: "project-id", envName: "RALPHX_PROJECT_ID" },
  { key: "workingDirectory", argName: "working-directory", envName: "RALPHX_WORKING_DIRECTORY" },
  { key: "contextType", argName: "context-type", envName: "RALPHX_CONTEXT_TYPE" },
  { key: "contextId", argName: "context-id", envName: "RALPHX_CONTEXT_ID" },
  { key: "leadSessionId", argName: "lead-session-id", envName: "RALPHX_LEAD_SESSION_ID" },
  { key: "tauriApiUrl", argName: "tauri-api-url", envName: "TAURI_API_URL" },
];

export function parseCliOptionFromArgs(
  args: readonly string[],
  optionName: string
): string | undefined {
  const inlinePrefix = `--${optionName}=`;
  const pairToken = `--${optionName}`;

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg.startsWith(inlinePrefix)) {
      return arg.slice(inlinePrefix.length);
    }
    if (arg === pairToken && index + 1 < args.length) {
      return args[index + 1];
    }
  }

  return undefined;
}

export function hydrateRalphxRuntimeEnvFromCli(
  args: readonly string[],
  env: NodeJS.ProcessEnv = process.env
): RuntimeContext {
  const context: RuntimeContext = {};

  for (const mapping of RUNTIME_ARG_ENV_MAPPINGS) {
    const cliValue = parseCliOptionFromArgs(args, mapping.argName);
    if (cliValue && cliValue.length > 0) {
      env[mapping.envName] = cliValue;
      context[mapping.key] = cliValue;
      continue;
    }

    const envValue = env[mapping.envName];
    if (envValue && envValue.length > 0) {
      context[mapping.key] = envValue;
    }
  }

  return context;
}
