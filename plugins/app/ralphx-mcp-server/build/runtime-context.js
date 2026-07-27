const RUNTIME_ARG_ENV_MAPPINGS = [
    { key: "agentType", argName: "agent-type", envName: "RALPHX_AGENT_TYPE" },
    { key: "agentProfile", argName: "agent-profile", envName: "RALPHX_AGENT_PROFILE" },
    { key: "taskId", argName: "task-id", envName: "RALPHX_TASK_ID" },
    { key: "taskState", argName: "task-state", envName: "RALPHX_TASK_STATE" },
    { key: "projectId", argName: "project-id", envName: "RALPHX_PROJECT_ID" },
    { key: "pipelineRole", argName: "pipeline-role", envName: "RALPHX_PIPELINE_ROLE" },
    {
        key: "skillDistillationBatchId",
        argName: "skill-distillation-batch-id",
        envName: "RALPHX_SKILL_DISTILLATION_BATCH_ID",
    },
    {
        key: "skillDistillationClaimToken",
        argName: "skill-distillation-claim-token",
        envName: "RALPHX_SKILL_DISTILLATION_CLAIM_TOKEN",
    },
    {
        key: "skillDistillationFingerprint",
        argName: "skill-distillation-fingerprint",
        envName: "RALPHX_SKILL_DISTILLATION_FINGERPRINT",
    },
    {
        key: "skillDistillationOutcomeIds",
        argName: "skill-distillation-outcome-ids",
        envName: "RALPHX_SKILL_DISTILLATION_OUTCOME_IDS",
    },
    { key: "workingDirectory", argName: "working-directory", envName: "RALPHX_WORKING_DIRECTORY" },
    { key: "contextType", argName: "context-type", envName: "RALPHX_CONTEXT_TYPE" },
    { key: "contextId", argName: "context-id", envName: "RALPHX_CONTEXT_ID" },
    { key: "conversationId", argName: "conversation-id", envName: "RALPHX_CONVERSATION_ID" },
    { key: "coordinationMode", argName: "coordination-mode", envName: "RALPHX_COORDINATION_MODE" },
    {
        key: "parentConversationId",
        argName: "parent-conversation-id",
        envName: "RALPHX_PARENT_CONVERSATION_ID",
    },
    { key: "agentRunId", argName: "agent-run-id", envName: "RALPHX_AGENT_RUN_ID" },
    { key: "leadSessionId", argName: "lead-session-id", envName: "RALPHX_LEAD_SESSION_ID" },
    { key: "tauriApiUrl", argName: "tauri-api-url", envName: "TAURI_API_URL" },
    { key: "traceDir", argName: "trace-dir", envName: "RALPHX_MCP_TRACE_DIR" },
];
export function parseCliOptionFromArgs(args, optionName) {
    return parseCliOptionsFromArgs(args, optionName)[0];
}
export function parseCliOptionsFromArgs(args, optionName) {
    const inlinePrefix = `--${optionName}=`;
    const pairToken = `--${optionName}`;
    const values = [];
    for (let index = 0; index < args.length; index += 1) {
        const arg = args[index];
        if (arg.startsWith(inlinePrefix)) {
            values.push(arg.slice(inlinePrefix.length));
            continue;
        }
        if (arg === pairToken && index + 1 < args.length) {
            values.push(args[index + 1]);
            index += 1;
        }
    }
    return values;
}
export function hydrateRalphxRuntimeEnvFromCli(args, env = process.env) {
    const context = {
        filesystemEnforced: parseCliOptionFromArgs(args, "filesystem-enforced") === "1",
    };
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
    const cliFilesystemReadRoots = parseCliOptionsFromArgs(args, "filesystem-read-root").filter((value) => value.length > 0);
    if (context.filesystemEnforced) {
        const serialized = JSON.stringify(cliFilesystemReadRoots);
        env.RALPHX_FILESYSTEM_READ_ROOTS = serialized;
        context.filesystemReadRoots = serialized;
    }
    else if (cliFilesystemReadRoots.length > 0) {
        const serialized = JSON.stringify(cliFilesystemReadRoots);
        env.RALPHX_FILESYSTEM_READ_ROOTS = serialized;
        context.filesystemReadRoots = serialized;
    }
    else {
        const envValue = env.RALPHX_FILESYSTEM_READ_ROOTS;
        if (envValue && envValue.length > 0) {
            context.filesystemReadRoots = envValue;
        }
    }
    return context;
}
export function buildArtifactMutationTransportHeaders(context) {
    const headers = {
        ...(buildRuntimeTransportHeaders(context) ?? {}),
    };
    if (context.contextType === "ideation" && context.contextId) {
        headers["X-RalphX-Caller-Session-Id"] = context.contextId;
    }
    Object.assign(headers, buildRuntimeIdentityTransportHeaders(context));
    return Object.keys(headers).length > 0 ? headers : undefined;
}
export function buildRuntimeIdentityTransportHeaders(context) {
    if (!context.agentRunId || !context.conversationId)
        return undefined;
    return {
        "x-ralphx-agent-run-id": context.agentRunId,
        "x-ralphx-conversation-id": context.conversationId,
    };
}
export function buildRuntimeTransportHeaders(context) {
    const conversationId = context.conversationId?.trim();
    return conversationId
        ? { "x-ralphx-conversation-id": conversationId }
        : undefined;
}
export function buildProjectSkillPipelineTransportHeaders(context) {
    const required = {
        agentName: context.agentType?.trim(),
        pipelineRole: context.pipelineRole?.trim(),
        projectId: context.projectId?.trim(),
        contextType: context.contextType?.trim(),
        contextId: context.contextId?.trim(),
        conversationId: context.conversationId?.trim(),
    };
    if (Object.values(required).some((value) => !value)) {
        return undefined;
    }
    return {
        "x-ralphx-agent-name": required.agentName,
        "x-ralphx-pipeline-role": required.pipelineRole,
        "x-ralphx-project-id": required.projectId,
        "x-ralphx-context-type": required.contextType,
        "x-ralphx-context-id": required.contextId,
        "x-ralphx-conversation-id": required.conversationId,
        ...(context.agentRunId?.trim()
            ? { "x-ralphx-agent-run-id": context.agentRunId.trim() }
            : {}),
        ...(context.taskId?.trim()
            ? { "x-ralphx-task-id": context.taskId.trim() }
            : {}),
        ...(context.skillDistillationBatchId?.trim()
            ? { "x-ralphx-skill-distillation-batch-id": context.skillDistillationBatchId.trim() }
            : {}),
        ...(context.skillDistillationClaimToken?.trim()
            ? {
                "x-ralphx-skill-distillation-claim-token": context.skillDistillationClaimToken.trim(),
            }
            : {}),
        ...(context.skillDistillationFingerprint?.trim()
            ? {
                "x-ralphx-skill-distillation-fingerprint": context.skillDistillationFingerprint.trim(),
            }
            : {}),
        ...(context.skillDistillationOutcomeIds?.trim()
            ? {
                "x-ralphx-skill-distillation-outcome-ids": context.skillDistillationOutcomeIds.trim(),
            }
            : {}),
    };
}
//# sourceMappingURL=runtime-context.js.map