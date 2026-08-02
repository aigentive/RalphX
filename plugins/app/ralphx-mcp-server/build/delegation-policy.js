import { loadCanonicalAgentDefinitionForProfile } from "./canonical-agent-metadata.js";
const DELEGATION_TOOL_NAMES = new Set([
    "delegate_start",
    "delegate_wait",
    "delegate_cancel",
    "delegate_park",
]);
function agentCanDelegate(agentType, agentProfile) {
    const definition = loadCanonicalAgentDefinitionForProfile(agentType, agentProfile);
    return Boolean(definition?.delegation?.allowed_targets?.length);
}
export function applyDelegationToolPolicy(toolNames, agentType, agentProfile) {
    if (agentCanDelegate(agentType, agentProfile)) {
        return toolNames;
    }
    return toolNames.filter((toolName) => !DELEGATION_TOOL_NAMES.has(toolName));
}
//# sourceMappingURL=delegation-policy.js.map