import { applyDelegationToolPolicy } from "./delegation-policy.js";
import { loadCanonicalMcpTools } from "./canonical-agent-metadata.js";
import { safeError } from "./redact.js";
import {
  ORCHESTRATOR_IDEATION,
  ORCHESTRATOR_IDEATION_READONLY,
  CHAT_TASK,
  CHAT_PROJECT,
  REVIEWER,
  REVIEW_CHAT,
  REVIEW_HISTORY,
  WORKER,
  CODER,
  SESSION_NAMER,
  PR_DESCRIBER,
  MERGER,
  PROJECT_ANALYZER,
  QA_PREP,
  QA_TESTER,
  ORCHESTRATOR,
  DEEP_RESEARCHER,
  MEMORY_MAINTAINER,
  MEMORY_CAPTURE,
  PLAN_CRITIC_COMPLETENESS,
  PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
  PLAN_VERIFIER,
  IDEATION_TEAM_LEAD,
  IDEATION_TEAM_MEMBER,
  WORKER_TEAM_LEAD,
  WORKER_TEAM_MEMBER,
  IDEATION_SPECIALIST_BACKEND,
  IDEATION_SPECIALIST_FRONTEND,
  IDEATION_SPECIALIST_INFRA,
  IDEATION_SPECIALIST_UX,
  IDEATION_SPECIALIST_CODE_QUALITY,
  IDEATION_SPECIALIST_PROMPT_QUALITY,
  IDEATION_SPECIALIST_INTENT,
  IDEATION_SPECIALIST_PIPELINE_SAFETY,
  IDEATION_SPECIALIST_STATE_MACHINE,
  IDEATION_CRITIC,
  IDEATION_ADVOCATE,
} from "./agentNames.js";

const CANONICAL_TOOL_ALLOWLIST_AGENTS: string[] = [
  ORCHESTRATOR_IDEATION,
  ORCHESTRATOR_IDEATION_READONLY,
  CHAT_TASK,
  CHAT_PROJECT,
  REVIEWER,
  REVIEW_CHAT,
  REVIEW_HISTORY,
  WORKER,
  CODER,
  SESSION_NAMER,
  PR_DESCRIBER,
  MERGER,
  PROJECT_ANALYZER,
  QA_PREP,
  QA_TESTER,
  ORCHESTRATOR,
  DEEP_RESEARCHER,
  MEMORY_MAINTAINER,
  MEMORY_CAPTURE,
  IDEATION_TEAM_LEAD,
  IDEATION_TEAM_MEMBER,
  IDEATION_SPECIALIST_BACKEND,
  IDEATION_SPECIALIST_FRONTEND,
  IDEATION_SPECIALIST_INFRA,
  IDEATION_SPECIALIST_UX,
  IDEATION_SPECIALIST_CODE_QUALITY,
  IDEATION_SPECIALIST_PROMPT_QUALITY,
  IDEATION_SPECIALIST_INTENT,
  IDEATION_SPECIALIST_PIPELINE_SAFETY,
  IDEATION_SPECIALIST_STATE_MACHINE,
  IDEATION_CRITIC,
  IDEATION_ADVOCATE,
  WORKER_TEAM_LEAD,
  WORKER_TEAM_MEMBER,
  PLAN_CRITIC_COMPLETENESS,
  PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
  PLAN_VERIFIER,
];

function loadCanonicalAllowlistOrThrow(agentType: string): string[] {
  const tools = loadCanonicalMcpTools(agentType);
  if (tools === undefined) {
    throw new Error(
      `[RalphX MCP] Missing canonical mcp_tools for TOOL_ALLOWLIST compatibility mirror agent "${agentType}"`
    );
  }
  return tools;
}

const TOOL_ALLOWLIST: Record<string, string[]> = Object.fromEntries(
  CANONICAL_TOOL_ALLOWLIST_AGENTS.map((agentType) => [
    agentType,
    loadCanonicalAllowlistOrThrow(agentType),
  ])
);
const LEGACY_TOOL_ALLOWLIST: Record<string, string[]> = {};

let currentAgentType = "";

export function setAgentType(agentType: string): void {
  currentAgentType = agentType;
}

export function getAgentType(): string {
  return currentAgentType || process.env.RALPHX_AGENT_TYPE || "";
}

const TOOL_NAME_PATTERN = /^[a-z][a-z0-9_]*$/;

export function parseAllowedToolsFromArgs(knownToolNames: string[]): string[] | undefined {
  for (const arg of process.argv) {
    if (arg.startsWith("--allowed-tools=")) {
      const value = arg.substring("--allowed-tools=".length);
      if (!value) return undefined;
      if (value === "__NONE__") return [];
      const tools = value.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
      const validated = tools.filter((t) => {
        if (!TOOL_NAME_PATTERN.test(t)) {
          safeError(`[RalphX MCP] WARN: Invalid tool name in --allowed-tools: "${t}" (skipped)`);
          return false;
        }
        return true;
      });
      const knownTools = new Set(knownToolNames);
      for (const t of validated) {
        if (!knownTools.has(t)) {
          safeError(`[RalphX MCP] WARN: --allowed-tools contains unknown tool "${t}" (not in ALL_TOOLS registry)`);
        }
      }
      return validated;
    }
  }
  return undefined;
}

export function getAllowedToolNames(knownToolNames: string[]): string[] {
  const agentType = getAgentType();

  const envAllowedTools = process.env.RALPHX_ALLOWED_MCP_TOOLS;
  if (envAllowedTools) {
    const tools = envAllowedTools.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
    return applyDelegationToolPolicy(tools, agentType);
  }

  const cliTools = parseAllowedToolsFromArgs(knownToolNames);
  if (cliTools !== undefined) {
    return applyDelegationToolPolicy(cliTools, agentType);
  }

  const canonicalTools = loadCanonicalMcpTools(agentType);
  if (canonicalTools !== undefined) {
    console.error(
      `[RalphX MCP] WARN: --allowed-tools not provided, using canonical agent capabilities`
    );
    return applyDelegationToolPolicy(canonicalTools, agentType);
  }

  const legacyTools = LEGACY_TOOL_ALLOWLIST[agentType];
  if (legacyTools) {
    console.error(
      `[RalphX MCP] WARN: --allowed-tools not provided, using fallback TOOL_ALLOWLIST (legacy only)`
    );
    return applyDelegationToolPolicy(legacyTools, agentType);
  }

  return [];
}

export function getToolsByAgent(knownToolNames: string[]): Record<string, string[]> {
  return {
    ...TOOL_ALLOWLIST,
    ...LEGACY_TOOL_ALLOWLIST,
    debug: knownToolNames,
  };
}

export function setLegacyToolAllowlistEntryForTest(
  agentType: string,
  tools: string[] | undefined
): void {
  if (tools === undefined) {
    delete LEGACY_TOOL_ALLOWLIST[agentType];
    return;
  }
  LEGACY_TOOL_ALLOWLIST[agentType] = [...tools];
}
