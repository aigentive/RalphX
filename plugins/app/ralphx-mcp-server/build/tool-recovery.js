function formatToolExamples(tool, limit = 1) {
    const examples = (tool.inputSchema?.examples ?? [])
        .slice(0, limit)
        .map((example) => {
        try {
            return JSON.stringify(example);
        }
        catch {
            return String(example);
        }
    })
        .filter((example) => example.length > 0);
    return examples;
}
export function getToolRecoveryHintFromRegistry(tools, toolName) {
    const tool = tools.find((candidate) => candidate.name === toolName);
    if (!tool) {
        return null;
    }
    switch (toolName) {
        case "report_verification_round": {
            const examples = formatToolExamples(tool);
            return [
                "Use this verifier-friendly helper for in-progress rounds on the PARENT ideation session.",
                "Do not pass session_id; the backend derives the canonical parent session from the active verification child context.",
                "You only provide round and generation; status=reviewing, in_progress=true, and current-round gaps come from the backend-owned run_verification_round state.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "complete_plan_verification": {
            const examples = formatToolExamples(tool);
            return [
                "Use this verifier-friendly helper for terminal verification updates on the PARENT ideation session.",
                "Do not pass session_id; the backend derives the canonical parent session from the active verification child context.",
                "You provide the terminal status and generation; in_progress=false is filled in automatically.",
                "The helper uses backend-owned current-round state from run_verification_round; do not try to pass delegate, timestamp, rescue, or wait bookkeeping through the model.",
                "External sessions cannot use status=skipped.",
                ...examples.map((example) => `Example terminal payload: ${example}`),
            ].join("\n");
        }
        case "get_plan_verification": {
            const examples = formatToolExamples(tool);
            return [
                "Call this before retrying report_verification_round or complete_plan_verification. The backend derives the canonical parent session from the active verification child context, so do not pass session_id.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "create_team_artifact": {
            const examples = formatToolExamples(tool);
            return [
                "Use the PARENT ideation session_id as the canonical target. If a verification child session id is passed, the backend remaps it to the parent automatically.",
                "Use this for general specialist findings and team summaries. It is not the typed verification-finding path.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "publish_verification_finding": {
            const examples = formatToolExamples(tool);
            return [
                "Use this for verification-path specialists and required verification critics.",
                "If session_id is omitted, the backend injects the current session context and remaps verification child sessions to the parent ideation session automatically.",
                "Publish one structured finding with critic, round, status, summary, and gaps.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "get_team_artifacts": {
            const examples = formatToolExamples(tool);
            return [
                "Read artifacts from the PARENT ideation session_id as the canonical target. If a verification child session id is passed, the backend remaps it to the parent automatically.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "run_verification_enrichment": {
            const examples = formatToolExamples(tool);
            return [
                "Use this as the backend-owned one-time enrichment driver.",
                "Do not pass session_id; the backend derives the canonical parent session from the active verification child context.",
                "You choose the enrichment specialists; the backend dispatches them, waits a bounded amount, and returns the latest typed findings plus delegate snapshots.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "run_verification_round": {
            const examples = formatToolExamples(tool);
            return [
                "Use this as the primary verifier round driver.",
                "Do not pass session_id; the backend derives the canonical parent session from the active verification child context.",
                "You choose the optional specialists; the backend dispatches them, runs the required critics, waits for bounded settlement, and returns structured required critic findings plus backend-owned merged_gaps.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "get_child_session_status": {
            const examples = formatToolExamples(tool);
            return [
                "When debugging a verification child, set include_recent_messages=true so you can inspect the last assistant/tool outputs.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "send_ideation_session_message": {
            const examples = formatToolExamples(tool);
            return [
                "When nudging a verifier/critic, repeat full invariant context: SESSION_ID, ROUND, critic/schema, and explicit parent-session target.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        case "claim_agent_task":
        case "complete_agent_task":
        case "update_agent_task": {
            const examples = formatToolExamples(tool);
            return [
                "A ledger with one meaningful task cannot be claimed, activated, or completed.",
                "For simple work, call update_agent_task with state=dropped on the lone task, then continue without the ledger.",
                "For non-trivial work, create multiple concrete tasks first, then claim the ready task.",
                ...examples.map((example) => `Example payload: ${example}`),
            ].join("\n");
        }
        default: {
            const examples = formatToolExamples(tool);
            if (examples.length === 0) {
                return null;
            }
            return examples.map((example) => `Example payload: ${example}`).join("\n");
        }
    }
}
export function formatToolErrorMessageFromRegistry(tools, toolName, message, details) {
    const repairHint = getToolRecoveryHintFromRegistry(tools, toolName);
    return (`ERROR: ${message}` +
        (details ? `\n\nDetails: ${details}` : "") +
        (repairHint ? `\n\nUsage hint for ${toolName}:\n${repairHint}` : ""));
}
//# sourceMappingURL=tool-recovery.js.map