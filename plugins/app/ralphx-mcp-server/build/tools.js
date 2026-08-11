/**
 * MCP tool definitions for RalphX
 * All tools are proxies that forward to Tauri backend via HTTP
 */
import { PLAN_TOOLS } from "./plan-tools.js";
import { WORKER_CONTEXT_TOOLS } from "./worker-context-tools.js";
import { STEP_TOOLS } from "./step-tools.js";
import { ISSUE_TOOLS } from "./issue-tools.js";
import { TICKET_ATTACHMENT_TOOLS } from "./ticket-attachment-tools.js";
import { FILESYSTEM_TOOLS } from "./filesystem-tools.js";
import { IDEATION_TOOLS } from "./ideation-tools.js";
import { WORKFLOW_TOOLS } from "./workflow-tools.js";
import { AGENT_WORKSPACE_TOOLS } from "./agent-workspace-tools.js";
import { AGENT_TASK_TOOLS } from "./agent-task-tools.js";
import { SUPPORT_TOOLS } from "./support-tools.js";
import { VALIDATION_TOOLS } from "./validation-tools.js";
import { AUTOMATION_SETUP_TOOLS } from "./automation-tools.js";
import { PERSONA_BUILDER_TOOLS } from "./persona-tools.js";
import { TEAM_TOOLS } from "./team-tools.js";
import { formatToolErrorMessageFromRegistry, getToolRecoveryHintFromRegistry, } from "./tool-recovery.js";
import { logToolsByAgent } from "./tool-debug.js";
import { getAllowedToolNames as resolveAllowedToolNames, getToolsByAgent as resolveToolsByAgent, parseAllowedToolsFromArgs as parseAllowedToolsFromKnownRegistry, } from "./tool-authorization.js";
export { setAgentType, getAgentType, } from "./tool-authorization.js";
/**
 * All available MCP tools
 * Tools are filtered based on RALPHX_AGENT_TYPE environment variable
 */
export const ALL_TOOLS = [
    ...FILESYSTEM_TOOLS,
    ...IDEATION_TOOLS,
    ...WORKFLOW_TOOLS,
    // ========================================================================
    // AGENT WORKSPACE TOOLS (workspace repair and publication)
    // ========================================================================
    ...AGENT_WORKSPACE_TOOLS,
    // ========================================================================
    // AGENT TASK TOOLS (native todo/dependency tracking)
    // ========================================================================
    ...AGENT_TASK_TOOLS,
    // ========================================================================
    // PLAN ARTIFACT TOOLS (ralphx-ideation agent)
    // ========================================================================
    ...PLAN_TOOLS,
    // ========================================================================
    // WORKER CONTEXT TOOLS (worker agent)
    // ========================================================================
    ...WORKER_CONTEXT_TOOLS,
    // ========================================================================
    // TASK VALIDATION / REVIEW EVIDENCE TOOLS
    // ========================================================================
    ...VALIDATION_TOOLS,
    // ========================================================================
    // AUTOMATION SETUP TOOLS
    // ========================================================================
    ...AUTOMATION_SETUP_TOOLS,
    // ========================================================================
    // PERSONA BUILDER TOOLS
    // ========================================================================
    ...PERSONA_BUILDER_TOOLS,
    // RX-native standing Team coordinator tools
    ...TEAM_TOOLS,
    // ========================================================================
    // STEP TOOLS (worker agent)
    // ========================================================================
    ...STEP_TOOLS,
    // ========================================================================
    // ISSUE TOOLS (worker + reviewer agents)
    // ========================================================================
    ...ISSUE_TOOLS,
    // ========================================================================
    // TICKET ATTACHMENT TOOLS (worker/coder execution agents)
    // ========================================================================
    ...TICKET_ATTACHMENT_TOOLS,
    ...SUPPORT_TOOLS,
];
const ALL_TOOL_NAMES = ALL_TOOLS.map((tool) => tool.name);
export function parseAllowedToolsFromArgs() {
    return parseAllowedToolsFromKnownRegistry(ALL_TOOL_NAMES);
}
export function getAllowedToolNames() {
    return resolveAllowedToolNames(ALL_TOOL_NAMES);
}
/**
 * Get filtered tools based on agent type
 * @returns Tools available to the current agent
 */
export function getFilteredTools() {
    const allowedNames = getAllowedToolNames();
    return ALL_TOOLS.filter((tool) => allowedNames.includes(tool.name));
}
/**
 * Check if a tool is allowed for the current agent type
 * @param toolName - Name of the tool to check
 * @returns true if allowed, false otherwise
 */
export function isToolAllowed(toolName) {
    const allowedNames = getAllowedToolNames();
    return allowedNames.includes(toolName);
}
/**
 * Get all tools regardless of agent type (for debugging)
 * @returns All available tools
 */
export function getAllTools() {
    return ALL_TOOLS;
}
/**
 * Get all tool names grouped by agent type (for debugging)
 * @returns Object mapping agent types to their allowed tools
 */
export function getToolsByAgent() {
    return resolveToolsByAgent(ALL_TOOL_NAMES);
}
/**
 * Return a compact repair hint for high-friction tools so weaker models can retry
 * with the expected payload shape instead of probing by trial and error.
 */
export function getToolRecoveryHint(toolName) {
    return getToolRecoveryHintFromRegistry(ALL_TOOLS, toolName);
}
/**
 * Format a backend error message with an optional tool-specific usage hint.
 */
export function formatToolErrorMessage(toolName, message, details) {
    return formatToolErrorMessageFromRegistry(ALL_TOOLS, toolName, message, details);
}
/**
 * Print all available tools to stderr (for debugging)
 * Call this to see what tools the MCP server can provide
 */
export function logAllTools() {
    logToolsByAgent(getToolsByAgent());
}
//# sourceMappingURL=tools.js.map