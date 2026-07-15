import { Tool } from "@modelcontextprotocol/sdk/types.js";

function formatToolExamples(tool: Tool, limit = 1): string[] {
  const examples = ((tool.inputSchema as { examples?: unknown[] } | undefined)?.examples ?? [])
    .slice(0, limit)
    .map((example) => {
      try {
        return JSON.stringify(example);
      } catch {
        return String(example);
      }
    })
    .filter((example) => example.length > 0);

  return examples;
}

export function getToolRecoveryHintFromRegistry(tools: Tool[], toolName: string): string | null {
  const tool = tools.find((candidate) => candidate.name === toolName);
  if (!tool) {
    return null;
  }

  switch (toolName) {
    case "complete_plan_verification": {
      return [
        "Call exactly once only after the current linked plan is implementation-ready.",
        "Pass an empty object. The backend derives and validates the active run, conversation, planning session, and exact current artifact.",
        "A stale, ordinary, failed, cancelled, or mismatched run cannot record proof.",
      ].join("\n");
    }
    case "get_plan_verification": {
      return [
        "Read the visible Verify Plan action status and exact-artifact proof.",
        "Pass session_id outside an ideation runtime; it is injected from context inside one.",
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
    case "get_team_artifacts": {
      const examples = formatToolExamples(tool);
      return [
        "Read artifacts from the PARENT ideation session_id as the canonical target. If a verification child session id is passed, the backend remaps it to the parent automatically.",
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

export function formatToolErrorMessageFromRegistry(
  tools: Tool[],
  toolName: string,
  message: string,
  details?: string
): string {
  const repairHint = getToolRecoveryHintFromRegistry(tools, toolName);
  return (
    `ERROR: ${message}` +
    (details ? `\n\nDetails: ${details}` : "") +
    (repairHint ? `\n\nUsage hint for ${toolName}:\n${repairHint}` : "")
  );
}
