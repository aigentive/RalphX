import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse as parseYaml } from "yaml";
const canonicalAgentDefinitionCache = new Map();
const SAFE_CANONICAL_AGENT_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const SAFE_CANONICAL_PROFILE_NAME = /^[a-z0-9]+(?:[_-][a-z0-9]+)*$/;
export function resolveRepoRoot() {
    let current = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../../");
    while (!fs.existsSync(path.join(current, "agents"))) {
        const parent = path.dirname(current);
        if (parent === current) {
            break;
        }
        current = parent;
    }
    return current;
}
export function canonicalAgentName(agentType) {
    const shortName = agentType.startsWith("ralphx:") ? agentType.slice("ralphx:".length) : agentType;
    switch (shortName) {
        case "orchestrator-ideation":
            return "ralphx-ideation";
        case "orchestrator-ideation-readonly":
            return "ralphx-ideation-readonly";
        case "ideation-advocate":
            return "ralphx-ideation-advocate";
        case "ideation-critic":
            return "ralphx-ideation-critic";
        case "ideation-specialist-backend":
            return "ralphx-ideation-specialist-backend";
        case "ideation-specialist-frontend":
            return "ralphx-ideation-specialist-frontend";
        case "ideation-specialist-infra":
            return "ralphx-ideation-specialist-infra";
        case "chat-task":
            return "ralphx-chat-task";
        case "chat-project":
            return "ralphx-chat-project";
        case "ralphx-worker":
            return "ralphx-execution-worker";
        case "ralphx-coder":
            return "ralphx-execution-coder";
        case "ralphx-reviewer":
            return "ralphx-execution-reviewer";
        case "ralphx-merger":
            return "ralphx-execution-merger";
        case "ralphx-orchestrator":
            return "ralphx-execution-orchestrator";
        case "ralphx-deep-researcher":
            return "ralphx-research-deep-researcher";
        case "project-analyzer":
            return "ralphx-project-analyzer";
        case "memory-capture":
            return "ralphx-memory-capture";
        case "memory-maintainer":
            return "ralphx-memory-maintainer";
        case "session-namer":
            return "ralphx-utility-session-namer";
        case "pr-describer":
            return "ralphx-utility-pr-describer";
        case "plan-complexity":
            return "ralphx-utility-plan-complexity";
        case "qa-prep":
            return "ralphx-qa-prep";
        case "qa-tester":
            return "ralphx-qa-executor";
        default:
            return shortName;
    }
}
export function clearCanonicalAgentDefinitionCache() {
    canonicalAgentDefinitionCache.clear();
}
function resolveCanonicalAgentDefinitionPath(canonicalName) {
    if (!SAFE_CANONICAL_AGENT_NAME.test(canonicalName)) {
        return null;
    }
    const agentsRoot = path.resolve(resolveRepoRoot(), "agents");
    const definitionPath = path.resolve(agentsRoot, canonicalName, "agent.yaml");
    const relative = path.relative(agentsRoot, definitionPath);
    if (relative.startsWith("..") || path.isAbsolute(relative)) {
        return null;
    }
    return definitionPath;
}
export function loadCanonicalAgentDefinition(agentType) {
    const canonicalName = canonicalAgentName(agentType);
    if (canonicalAgentDefinitionCache.has(canonicalName)) {
        return canonicalAgentDefinitionCache.get(canonicalName) ?? null;
    }
    const definitionPath = resolveCanonicalAgentDefinitionPath(canonicalName);
    if (!definitionPath) {
        canonicalAgentDefinitionCache.set(canonicalName, null);
        return null;
    }
    try {
        const raw = fs.readFileSync(definitionPath, "utf8");
        const parsed = parseYaml(raw);
        const definition = parsed && parsed.name === canonicalName ? parsed : null;
        canonicalAgentDefinitionCache.set(canonicalName, definition);
        return definition;
    }
    catch {
        canonicalAgentDefinitionCache.set(canonicalName, null);
        return null;
    }
}
export function loadCanonicalAgentDefinitionForProfile(agentType, agentProfile) {
    const definition = loadCanonicalAgentDefinition(agentType);
    if (!definition || !agentProfile) {
        return definition;
    }
    if (!SAFE_CANONICAL_PROFILE_NAME.test(agentProfile)) {
        return null;
    }
    const profile = definition.profiles?.[agentProfile];
    if (!profile) {
        return null;
    }
    const profileTools = profile.capabilities?.mcp_tools;
    return {
        ...definition,
        capabilities: profileTools && profileTools.length > 0
            ? { ...definition.capabilities, mcp_tools: [...profileTools] }
            : definition.capabilities,
        delegation: profile.delegation
            ? { allowed_targets: [...(profile.delegation.allowed_targets ?? [])] }
            : definition.delegation,
    };
}
export function loadCanonicalMcpTools(agentType, agentProfile) {
    const definition = loadCanonicalAgentDefinitionForProfile(agentType, agentProfile);
    const tools = definition?.capabilities?.mcp_tools;
    return tools ? [...tools] : undefined;
}
//# sourceMappingURL=canonical-agent-metadata.js.map