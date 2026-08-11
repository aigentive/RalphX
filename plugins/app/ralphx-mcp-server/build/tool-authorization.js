import { applyDelegationToolPolicy } from "./delegation-policy.js";
import { applyTeamToolPolicy } from "./team-tool-policy.js";
import { loadCanonicalMcpTools } from "./canonical-agent-metadata.js";
import { safeError } from "./redact.js";
import { ORCHESTRATOR_IDEATION, ORCHESTRATOR_IDEATION_READONLY, CHAT_TASK, CHAT_PROJECT, REVIEWER, REVIEW_CHAT, REVIEW_HISTORY, WORKER, GENERAL_EXPLORER, GENERAL_WORKER, PR_REVIEWER, AGENT_WORKSPACE_REPAIR, CODER, SESSION_NAMER, PR_DESCRIBER, WORKSPACE_REVIEWER, AUTOMATION_SETUP, PLAN_COMPLEXITY_ASSESSOR, MERGER, PROJECT_ANALYZER, QA_PREP, QA_TESTER, ORCHESTRATOR, DEEP_RESEARCHER, MEMORY_MAINTAINER, MEMORY_CAPTURE, IDEATION_SPECIALIST_BACKEND, IDEATION_SPECIALIST_FRONTEND, IDEATION_SPECIALIST_INFRA, IDEATION_CRITIC, IDEATION_ADVOCATE, } from "./agentNames.js";
const CANONICAL_TOOL_ALLOWLIST_AGENTS = [
    ORCHESTRATOR_IDEATION,
    ORCHESTRATOR_IDEATION_READONLY,
    CHAT_TASK,
    CHAT_PROJECT,
    REVIEWER,
    REVIEW_CHAT,
    REVIEW_HISTORY,
    WORKER,
    GENERAL_EXPLORER,
    GENERAL_WORKER,
    PR_REVIEWER,
    AGENT_WORKSPACE_REPAIR,
    CODER,
    SESSION_NAMER,
    PR_DESCRIBER,
    WORKSPACE_REVIEWER,
    AUTOMATION_SETUP,
    PLAN_COMPLEXITY_ASSESSOR,
    MERGER,
    PROJECT_ANALYZER,
    QA_PREP,
    QA_TESTER,
    ORCHESTRATOR,
    DEEP_RESEARCHER,
    MEMORY_MAINTAINER,
    MEMORY_CAPTURE,
    IDEATION_SPECIALIST_BACKEND,
    IDEATION_SPECIALIST_FRONTEND,
    IDEATION_SPECIALIST_INFRA,
    IDEATION_CRITIC,
    IDEATION_ADVOCATE,
];
function loadCanonicalAllowlistOrThrow(agentType) {
    const tools = loadCanonicalMcpTools(agentType);
    if (tools === undefined) {
        throw new Error(`[RalphX MCP] Missing canonical mcp_tools for TOOL_ALLOWLIST compatibility mirror agent "${agentType}"`);
    }
    return tools;
}
const TOOL_ALLOWLIST = Object.fromEntries(CANONICAL_TOOL_ALLOWLIST_AGENTS.map((agentType) => [
    agentType,
    loadCanonicalAllowlistOrThrow(agentType),
]));
const LEGACY_TOOL_ALLOWLIST = {};
let currentAgentType = "";
export function setAgentType(agentType) {
    currentAgentType = agentType;
}
export function getAgentType() {
    return currentAgentType || process.env.RALPHX_AGENT_TYPE || "";
}
export function getAgentProfile() {
    const profile = process.env.RALPHX_AGENT_PROFILE;
    return profile && profile.length > 0 ? profile : undefined;
}
const TOOL_NAME_PATTERN = /^[a-z][a-z0-9_]*$/;
const WORKFLOW_TOOL_NAMES = new Set([
    "create_agent_workflow_script",
    "start_agent_workflow_run",
    "get_agent_workflow_run",
    "pause_agent_workflow_run",
    "resume_agent_workflow_run",
    "cancel_agent_workflow_run",
]);
function applyWorkflowToolPolicy(tools) {
    if (process.env.RALPHX_COORDINATION_MODE === "rx_native_workflow") {
        return tools;
    }
    return tools.filter((tool) => !WORKFLOW_TOOL_NAMES.has(tool));
}
function applyRuntimeToolPolicies(tools, agentType, agentProfile) {
    return applyTeamToolPolicy(applyWorkflowToolPolicy(applyDelegationToolPolicy(tools, agentType, agentProfile)));
}
export function parseAllowedToolsFromArgs(knownToolNames) {
    for (const arg of process.argv) {
        if (arg.startsWith("--allowed-tools=")) {
            const value = arg.substring("--allowed-tools=".length);
            if (!value)
                return undefined;
            if (value === "__NONE__")
                return [];
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
export function getAllowedToolNames(knownToolNames) {
    const agentType = getAgentType();
    const agentProfile = getAgentProfile();
    const envAllowedTools = process.env.RALPHX_ALLOWED_MCP_TOOLS;
    if (envAllowedTools) {
        const tools = envAllowedTools.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
        return applyRuntimeToolPolicies(tools, agentType, agentProfile);
    }
    const cliTools = parseAllowedToolsFromArgs(knownToolNames);
    if (cliTools !== undefined) {
        return applyRuntimeToolPolicies(cliTools, agentType, agentProfile);
    }
    const canonicalTools = loadCanonicalMcpTools(agentType, agentProfile);
    if (canonicalTools !== undefined) {
        console.error(`[RalphX MCP] WARN: --allowed-tools not provided, using canonical agent capabilities`);
        return applyRuntimeToolPolicies(canonicalTools, agentType, agentProfile);
    }
    const legacyTools = LEGACY_TOOL_ALLOWLIST[agentType];
    if (legacyTools) {
        console.error(`[RalphX MCP] WARN: --allowed-tools not provided, using fallback TOOL_ALLOWLIST (legacy only)`);
        return applyRuntimeToolPolicies(legacyTools, agentType, agentProfile);
    }
    return [];
}
export function getToolsByAgent(knownToolNames) {
    return {
        ...TOOL_ALLOWLIST,
        ...LEGACY_TOOL_ALLOWLIST,
        debug: knownToolNames,
    };
}
export function setLegacyToolAllowlistEntryForTest(agentType, tools) {
    if (tools === undefined) {
        delete LEGACY_TOOL_ALLOWLIST[agentType];
        return;
    }
    LEGACY_TOOL_ALLOWLIST[agentType] = [...tools];
}
//# sourceMappingURL=tool-authorization.js.map