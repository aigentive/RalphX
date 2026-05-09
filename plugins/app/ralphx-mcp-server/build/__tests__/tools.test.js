/**
 * Unit tests for MCP tool definitions and authorization logic
 * Tests agent team coordination features
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { getAllowedToolNames, getFilteredTools, getToolsByAgent, isToolAllowed, setAgentType, getAllTools, getToolRecoveryHint, formatToolErrorMessage, parseAllowedToolsFromArgs, } from '../tools.js';
import { canonicalAgentName, loadCanonicalMcpTools } from '../canonical-agent-metadata.js';
import { setLegacyToolAllowlistEntryForTest } from '../tool-authorization.js';
import { PLAN_TOOLS } from '../plan-tools.js';
import { IDEATION_TEAM_LEAD, IDEATION_TEAM_MEMBER, WORKER_TEAM_LEAD, WORKER_TEAM_MEMBER, ORCHESTRATOR_IDEATION, ORCHESTRATOR_IDEATION_READONLY, IDEATION_SPECIALIST_BACKEND, IDEATION_SPECIALIST_FRONTEND, IDEATION_SPECIALIST_INFRA, IDEATION_SPECIALIST_CODE_QUALITY, IDEATION_SPECIALIST_UX, IDEATION_SPECIALIST_PROMPT_QUALITY, IDEATION_SPECIALIST_INTENT, IDEATION_SPECIALIST_PIPELINE_SAFETY, IDEATION_SPECIALIST_STATE_MACHINE, IDEATION_CRITIC, IDEATION_ADVOCATE, PLAN_VERIFIER, PLAN_CRITIC_COMPLETENESS, PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY, REVIEWER, WORKER, MERGER, CHAT_PROJECT, } from '../agentNames.js';
function toolsByAgent() {
    return getToolsByAgent();
}
describe('getAllowedToolNames', () => {
    beforeEach(() => {
        // Clear env var before each test
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    afterEach(() => {
        // Clean up env var after each test
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    it('should return parsed list when RALPHX_ALLOWED_MCP_TOOLS env var is set', () => {
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan,create_team_artifact';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan', 'create_team_artifact']);
    });
    it('should handle spaces in env var', () => {
        process.env.RALPHX_ALLOWED_MCP_TOOLS = '  get_session_plan  ,  create_team_artifact  ';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan', 'create_team_artifact']);
    });
    it('should handle trailing commas in env var', () => {
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan,create_team_artifact,';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan', 'create_team_artifact']);
    });
    it('should filter out empty entries in env var', () => {
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan,,create_team_artifact,  ,';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan', 'create_team_artifact']);
    });
    it('should return legacy fallback entry when env var is unset and agent type lacks canonical metadata', () => {
        const originalTools = toolsByAgent()['legacy-fallback-agent'];
        setLegacyToolAllowlistEntryForTest('legacy-fallback-agent', ['get_session_plan']);
        try {
            setAgentType('legacy-fallback-agent');
            const tools = getAllowedToolNames();
            expect(tools).toEqual(['get_session_plan']);
        }
        finally {
            setLegacyToolAllowlistEntryForTest('legacy-fallback-agent', originalTools);
        }
    });
    it('should return empty array when env var is unset and agent type is unknown', () => {
        setAgentType('unknown-agent-type');
        const tools = getAllowedToolNames();
        expect(tools).toEqual([]);
    });
    it('should return empty array when env var is empty string', () => {
        process.env.RALPHX_ALLOWED_MCP_TOOLS = '';
        const tools = getAllowedToolNames();
        expect(tools).toEqual([]);
    });
    it('should prioritize env var over agent type allowlist', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan';
        const tools = getAllowedToolNames();
        // Should return env var list, not agent type allowlist
        expect(tools).toEqual(['get_session_plan']);
        expect(tools).not.toEqual(toolsByAgent()[IDEATION_TEAM_LEAD]);
    });
    it('should strip delegation tools from env override for non-delegating agents', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'delegate_start,get_session_plan,delegate_wait';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan']);
    });
    it('prefers canonical mcp_tools when available', () => {
        setAgentType('qa-prep');
        const tools = getAllowedToolNames();
        expect(tools).toEqual(loadCanonicalMcpTools('qa-prep'));
        expect(tools).toContain('fs_read_file');
        expect(tools).toContain('fs_grep');
    });
    it('rejects canonical agent path traversal attempts', () => {
        expect(loadCanonicalMcpTools('../secrets')).toBeUndefined();
    });
    it('treats delegation-only canonical mcp_tools as canonical instead of missing', () => {
        setAgentType('qa-tester');
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['delegate_start', 'delegate_wait', 'delegate_cancel']);
        expect(loadCanonicalMcpTools('qa-tester')).toEqual([
            'delegate_start',
            'delegate_wait',
            'delegate_cancel',
        ]);
    });
    it('resolves the PR describer legacy alias to canonical metadata', () => {
        setAgentType('pr-describer');
        expect(canonicalAgentName('pr-describer')).toBe('ralphx-utility-pr-describer');
        expect(getAllowedToolNames()).toEqual(loadCanonicalMcpTools('pr-describer'));
        expect(getAllowedToolNames()).toContain('submit_agent_workspace_pr_description');
    });
});
describe('getToolRecoveryHint', () => {
    it('does not expose recovery guidance for removed update_plan_verification', () => {
        expect(getToolRecoveryHint('update_plan_verification')).toBeNull();
    });
    it('returns narrower verifier-helper guidance for report_verification_round', () => {
        const hint = getToolRecoveryHint('report_verification_round');
        expect(hint).toContain('verifier-friendly helper');
        expect(hint).toContain('Do not pass session_id');
        expect(hint).toContain('current-round gaps come from the backend-owned run_verification_round state');
        expect(hint).toContain('Example payload:');
    });
    it('returns narrower verifier-helper guidance for complete_plan_verification', () => {
        const hint = getToolRecoveryHint('complete_plan_verification');
        expect(hint).toContain('terminal verification updates');
        expect(hint).toContain('Do not pass session_id');
        expect(hint).toContain('in_progress=false is filled in automatically');
        expect(hint).toContain('do not try to pass delegate, timestamp, rescue, or wait bookkeeping');
        expect(hint).toContain('External sessions cannot use status=skipped');
    });
    it('keeps verifier recovery guidance surface-local for get_plan_verification', () => {
        const hint = getToolRecoveryHint('get_plan_verification');
        expect(hint).toContain('retrying report_verification_round or complete_plan_verification');
        expect(hint).not.toContain('update_plan_verification');
    });
    it('returns enrichment guidance for run_verification_enrichment', () => {
        const hint = getToolRecoveryHint('run_verification_enrichment');
        expect(hint).toContain('backend-owned one-time enrichment driver');
        expect(hint).toContain('You choose the enrichment specialists');
    });
    it('returns round-driver guidance for run_verification_round', () => {
        const hint = getToolRecoveryHint('run_verification_round');
        expect(hint).toContain('primary verifier round driver');
        expect(hint).toContain('structured required critic findings');
        expect(hint).toContain('You choose the optional specialists');
    });
    it('returns verifier-debugging guidance for get_child_session_status', () => {
        const hint = getToolRecoveryHint('get_child_session_status');
        expect(hint).toContain('include_recent_messages=true');
        expect(hint).toContain('Example payload:');
    });
    it('returns invariant-context guidance for send_ideation_session_message', () => {
        const hint = getToolRecoveryHint('send_ideation_session_message');
        expect(hint).toContain('SESSION_ID, ROUND, critic/schema');
        expect(hint).toContain('Example payload:');
    });
    it('returns null for an unknown tool', () => {
        expect(getToolRecoveryHint('not_a_real_tool')).toBeNull();
    });
    it('keeps plan edit caller identity off the live tool schema', () => {
        const updateTool = PLAN_TOOLS.find((t) => t.name === 'update_plan_artifact');
        const editTool = PLAN_TOOLS.find((t) => t.name === 'edit_plan_artifact');
        expect(updateTool).toBeDefined();
        expect(editTool).toBeDefined();
        expect(updateTool?.inputSchema.properties).not.toHaveProperty('caller_session_id');
        expect(editTool?.inputSchema.properties).not.toHaveProperty('caller_session_id');
        expect(updateTool?.description).toContain('derived automatically from live app context');
        expect(editTool?.description).toContain('derived automatically from live app context');
    });
});
describe('buildAppendTaskToIdeationPlanPayload', () => {
    it('maps MCP snake_case arguments to the camelCase Tauri payload', () => {
        expect(buildAppendTaskToIdeationPlanPayload({
            project_id: 'project-1',
            session_id: 'session-1',
            title: 'Add follow-up coverage',
            description: 'Cover the waiting-on-PR append path.',
            steps: ['Add regression test', 'Implement fix'],
            acceptance_criteria: ['Waiting-on-PR plans accept the task'],
            depends_on_task_ids: ['task-1'],
            priority: 4,
            source_conversation_id: 'conversation-1',
            source_message_id: 'message-1',
        })).toEqual({
            projectId: 'project-1',
            sessionId: 'session-1',
            title: 'Add follow-up coverage',
            description: 'Cover the waiting-on-PR append path.',
            steps: ['Add regression test', 'Implement fix'],
            acceptanceCriteria: ['Waiting-on-PR plans accept the task'],
            dependsOnTaskIds: ['task-1'],
            priority: 4,
            sourceConversationId: 'conversation-1',
            sourceMessageId: 'message-1',
        });
    });
    it('omits optional fields that were not provided', () => {
        expect(buildAppendTaskToIdeationPlanPayload({
            project_id: 'project-1',
            session_id: 'session-1',
            title: 'Small follow-up',
            steps: [],
            acceptance_criteria: [],
        })).toEqual({
            projectId: 'project-1',
            sessionId: 'session-1',
            title: 'Small follow-up',
            steps: [],
            acceptanceCriteria: [],
        });
    });
});
describe('formatToolErrorMessage', () => {
    it('appends details and a usage hint for known high-friction tools', () => {
        const text = formatToolErrorMessage('report_verification_round', 'Verification round state is stale.', 'Use the parent session id instead.');
        expect(text).toContain('ERROR: Verification round state is stale.');
        expect(text).toContain('Details: Use the parent session id instead.');
        expect(text).toContain('Usage hint for report_verification_round:');
        expect(text).toContain('Example payload:');
    });
    it('leaves unknown tools without a usage-hint section', () => {
        const text = formatToolErrorMessage('not_a_real_tool', 'boom');
        expect(text).toBe('ERROR: boom');
    });
});
describe('tool input schemas', () => {
    it('do not expose top-level JSON schema combinators rejected by Claude tools', () => {
        for (const tool of getAllTools()) {
            expect(tool.inputSchema, `${tool.name} inputSchema`).not.toHaveProperty('oneOf');
            expect(tool.inputSchema, `${tool.name} inputSchema`).not.toHaveProperty('allOf');
            expect(tool.inputSchema, `${tool.name} inputSchema`).not.toHaveProperty('anyOf');
        }
    });
});
describe('getFilteredTools', () => {
    beforeEach(() => {
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    afterEach(() => {
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    it('should return correct tool set for ralphx-ideation-team-lead', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        // Should include team coordination tools
        expect(toolNames).toContain('request_team_plan');
        expect(toolNames).toContain('request_teammate_spawn');
        expect(toolNames).toContain('create_team_artifact');
        expect(toolNames).toContain('get_team_artifacts');
        expect(toolNames).toContain('get_team_session_state');
        expect(toolNames).toContain('save_team_session_state');
        // Should include ideation tools
        expect(toolNames).toContain('create_task_proposal');
        expect(toolNames).toContain('update_task_proposal');
        expect(toolNames).toContain('get_session_plan');
        expect(toolNames).not.toContain('update_plan_verification');
        // Should match allowlist count
        expect(tools.length).toBe(toolsByAgent()[IDEATION_TEAM_LEAD].length);
    });
    it('should return only allowed tools for ideation-team-member (read-only)', () => {
        setAgentType(IDEATION_TEAM_MEMBER);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        // Should include artifact tools
        expect(toolNames).toContain('create_team_artifact');
        expect(toolNames).toContain('get_team_artifacts');
        // Should include read-only access tools
        expect(toolNames).toContain('get_session_plan');
        expect(toolNames).toContain('list_session_proposals');
        expect(toolNames).toContain('get_artifact');
        // Should NOT include lead-only tools
        expect(toolNames).not.toContain('request_team_plan');
        expect(toolNames).not.toContain('request_teammate_spawn');
        expect(toolNames).not.toContain('create_task_proposal');
        expect(toolNames).not.toContain('save_team_session_state');
        // Should match allowlist count
        expect(tools.length).toBe(toolsByAgent()[IDEATION_TEAM_MEMBER].length);
    });
    it('should return correct tool set for worker-team-member', () => {
        setAgentType(WORKER_TEAM_MEMBER);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        // Should include artifact tools (document decisions)
        expect(toolNames).toContain('create_team_artifact');
        expect(toolNames).toContain('get_team_artifacts');
        // Should include worker step tools
        expect(toolNames).toContain('start_step');
        expect(toolNames).toContain('complete_step');
        expect(toolNames).toContain('get_task_context');
        // Should NOT include lead-only tools
        expect(toolNames).not.toContain('request_team_plan');
        expect(toolNames).not.toContain('request_teammate_spawn');
        expect(toolNames).not.toContain('save_team_session_state');
        // Should match allowlist count
        expect(tools.length).toBe(toolsByAgent()[WORKER_TEAM_MEMBER].length);
    });
    it('should keep ralphx-ideation on start/observe/stop verification tools only', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toContain('create_child_session');
        expect(toolNames).toContain('get_plan_verification');
        expect(toolNames).toContain('stop_verification');
        expect(toolNames).not.toContain('update_plan_verification');
        expect(toolNames).not.toContain('report_verification_round');
        expect(toolNames).not.toContain('complete_plan_verification');
    });
    it('should keep project chat off internal ideation launch and mutation tools', () => {
        setAgentType(CHAT_PROJECT);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toContain('suggest_task');
        expect(toolNames).toContain('list_tasks');
        expect(toolNames).not.toContain('start_ideation_session');
        expect(toolNames).not.toContain('create_child_session');
        expect(toolNames).not.toContain('create_task_proposal');
        expect(toolNames).not.toContain('update_plan_artifact');
    });
    it('should scope ralphx-plan-verifier to the narrower verification helpers', () => {
        setAgentType('ralphx-plan-verifier');
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toContain('fs_read_file');
        expect(toolNames).toContain('fs_list_dir');
        expect(toolNames).toContain('fs_grep');
        expect(toolNames).toContain('fs_glob');
        expect(toolNames).toContain('run_verification_enrichment');
        expect(toolNames).toContain('run_verification_round');
        expect(toolNames).toContain('complete_plan_verification');
        expect(toolNames).toContain('get_plan_verification');
        expect(toolNames).not.toContain('send_ideation_session_message');
        expect(toolNames).not.toContain('delegate_start');
        expect(toolNames).not.toContain('delegate_wait');
        expect(toolNames).not.toContain('delegate_cancel');
        expect(toolNames).not.toContain('get_session_messages');
        expect(toolNames).not.toContain('update_plan_verification');
        expect(toolNames).not.toContain('get_team_artifacts');
        expect(toolNames).not.toContain('get_artifact');
        expect(toolNames).not.toContain('assess_verification_round');
        expect(toolNames).not.toContain('run_required_verification_critic_round');
        expect(toolNames).not.toContain('await_verification_round_settlement');
    });
    it('should expose qa prep filesystem tools plus delegation bridge tools', () => {
        setAgentType('qa-prep');
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toEqual([
            'fs_read_file',
            'fs_list_dir',
            'fs_grep',
            'fs_glob',
            'delegate_start',
            'delegate_wait',
            'delegate_cancel',
        ]);
    });
    it('should return no tools for unknown agent type', () => {
        setAgentType('unknown-agent-type');
        const tools = getFilteredTools();
        expect(tools).toEqual([]);
    });
    it('should return only env var tools when env var is set', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan,create_team_artifact';
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        // Check contents without caring about order
        expect(toolNames).toContain('get_session_plan');
        expect(toolNames).toContain('create_team_artifact');
        expect(tools.length).toBe(2);
    });
});
describe('isToolAllowed', () => {
    beforeEach(() => {
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    afterEach(() => {
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    it('should return true for allowed tool', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        expect(isToolAllowed('request_team_plan')).toBe(true);
        expect(isToolAllowed('create_task_proposal')).toBe(true);
    });
    it('should return false for disallowed tool', () => {
        setAgentType(IDEATION_TEAM_MEMBER);
        expect(isToolAllowed('request_team_plan')).toBe(false);
        expect(isToolAllowed('create_task_proposal')).toBe(false);
    });
    it('should return false for unknown tool', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        expect(isToolAllowed('nonexistent_tool')).toBe(false);
    });
    it('should respect env var override for allowed tool', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan';
        expect(isToolAllowed('get_session_plan')).toBe(true);
        expect(isToolAllowed('request_team_plan')).toBe(false); // Not in env var
    });
    it('should respect env var override for disallowed tool', () => {
        setAgentType(IDEATION_TEAM_MEMBER);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'request_team_plan'; // Normally not allowed
        expect(isToolAllowed('request_team_plan')).toBe(true);
    });
});
describe('New team tool definitions', () => {
    const allTools = getAllTools();
    describe('request_team_plan', () => {
        const tool = allTools.find((t) => t.name === 'request_team_plan');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('process');
            expect(tool?.inputSchema.properties).toHaveProperty('teammates');
            expect(tool?.inputSchema.required).toContain('process');
            expect(tool?.inputSchema.required).toContain('teammates');
        });
        it('should have teammates array with correct item schema', () => {
            const teammates = tool?.inputSchema.properties?.teammates;
            expect(teammates).toBeDefined();
            expect(teammates.type).toBe('array');
            expect(teammates.items).toBeDefined();
            expect(teammates.items.type).toBe('object');
            expect(teammates.items.properties).toHaveProperty('role');
            expect(teammates.items.properties).toHaveProperty('tools');
            expect(teammates.items.properties).toHaveProperty('mcp_tools');
            expect(teammates.items.properties).toHaveProperty('model');
            expect(teammates.items.properties).toHaveProperty('prompt_summary');
        });
    });
    describe('request_teammate_spawn', () => {
        const tool = allTools.find((t) => t.name === 'request_teammate_spawn');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('role');
            expect(tool?.inputSchema.properties).toHaveProperty('prompt');
            expect(tool?.inputSchema.properties).toHaveProperty('model');
            expect(tool?.inputSchema.properties).toHaveProperty('tools');
            expect(tool?.inputSchema.properties).toHaveProperty('mcp_tools');
            expect(tool?.inputSchema.required).toContain('role');
            expect(tool?.inputSchema.required).toContain('prompt');
            expect(tool?.inputSchema.required).toContain('model');
            expect(tool?.inputSchema.required).toContain('tools');
            expect(tool?.inputSchema.required).toContain('mcp_tools');
        });
        it('should have model enum constraint', () => {
            const model = tool?.inputSchema.properties?.model;
            expect(model).toBeDefined();
            expect(model.enum).toEqual(['haiku', 'sonnet', 'opus']);
        });
    });
    describe('create_team_artifact', () => {
        const tool = allTools.find((t) => t.name === 'create_team_artifact');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('session_id');
            expect(tool?.inputSchema.properties).toHaveProperty('title');
            expect(tool?.inputSchema.properties).toHaveProperty('content');
            expect(tool?.inputSchema.properties).toHaveProperty('artifact_type');
            expect(tool?.inputSchema.required).toContain('session_id');
            expect(tool?.inputSchema.required).toContain('title');
            expect(tool?.inputSchema.required).toContain('content');
            expect(tool?.inputSchema.required).toContain('artifact_type');
        });
        it('should have artifact_type enum constraint', () => {
            const artifactType = tool?.inputSchema.properties?.artifact_type;
            expect(artifactType).toBeDefined();
            expect(artifactType.enum).toEqual(['TeamResearch', 'TeamAnalysis', 'TeamSummary']);
        });
        it('should document parent-session targeting for verification flows', () => {
            expect(tool?.description).toContain('PARENT ideation session_id');
            expect(tool?.description).toContain('backend remaps it to the parent ideation session automatically');
            expect(tool?.description).toContain('general team-artifact path');
            expect(tool?.inputSchema.properties?.session_id?.description).toContain('auto-remapped to that parent');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'parent-session-id',
                artifact_type: 'TeamResearch',
            });
        });
    });
    describe('publish_verification_finding', () => {
        const tool = allTools.find((t) => t.name === 'publish_verification_finding');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should expose the typed verification payload', () => {
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('critic');
            expect(tool?.inputSchema.properties).toHaveProperty('round');
            expect(tool?.inputSchema.properties).toHaveProperty('status');
            expect(tool?.inputSchema.properties).toHaveProperty('summary');
            expect(tool?.inputSchema.properties).toHaveProperty('gaps');
            expect(tool?.inputSchema.required).toEqual(expect.arrayContaining(['critic', 'round', 'status', 'summary', 'gaps']));
            expect(tool?.description).toContain('typed verification finding');
            expect(tool?.description).toContain('delegated verification specialists/critics');
            expect(tool?.inputSchema.properties?.session_id?.description).toContain('Optional parent ideation session ID');
            expect(tool?.inputSchema.properties?.session_id?.description).toContain('including delegated verification specialists and critics');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                critic: 'completeness',
                status: 'partial',
            });
        });
    });
    describe('get_team_artifacts', () => {
        const tool = allTools.find((t) => t.name === 'get_team_artifacts');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('session_id');
            expect(tool?.inputSchema.required).toContain('session_id');
        });
        it('should document round-oriented verification lookup guidance', () => {
            expect(tool?.description).toContain('PARENT ideation session_id');
            expect(tool?.description).toContain('backend remaps it to the parent ideation session automatically');
            expect(tool?.description).toContain('raw artifact listing surface');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'parent-session-id',
            });
        });
    });
    describe('update_plan_verification', () => {
        it('should be absent from the live public tool registry', () => {
            expect(PLAN_TOOLS.find((t) => t.name === 'update_plan_verification')).toBeUndefined();
            expect(getAllTools().map((tool) => tool.name)).not.toContain('update_plan_verification');
        });
    });
    describe('report_verification_round', () => {
        const tool = PLAN_TOOLS.find((t) => t.name === 'report_verification_round');
        it('should expose the verifier-friendly round helper with fixed semantics', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('Verifier-friendly helper');
            expect(tool?.description).toContain('Do not pass session_id');
            expect(tool?.description).toContain('response is authoritative for next-step control flow');
            expect(tool?.description).toContain('actionable plan feedback');
            expect(tool?.description).not.toContain('update_plan_verification');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                round: 1,
                generation: 3,
            });
            expect(tool?.inputSchema.properties).not.toHaveProperty('session_id');
            expect(tool?.inputSchema.required).toEqual(['round', 'generation']);
        });
    });
    describe('get_plan_verification', () => {
        const tool = PLAN_TOOLS.find((t) => t.name === 'get_plan_verification');
        it('should derive the parent session automatically for verifier-owned reads', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('do not pass session_id');
            expect(tool?.inputSchema.properties).toEqual({});
            expect((tool?.inputSchema).examples?.[0]).toEqual({});
            expect(tool?.inputSchema.required).toEqual([]);
        });
    });
    describe('complete_plan_verification', () => {
        const tool = PLAN_TOOLS.find((t) => t.name === 'complete_plan_verification');
        it('should expose the verifier-friendly terminal helper with fixed semantics', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('Verifier-friendly helper');
            expect(tool?.description).toContain('Do not pass session_id');
            expect(tool?.description).toContain('uses the backend-owned current round state');
            expect(tool?.description).toContain('Do not call it immediately after an actionable needs_revision round report');
            expect(tool?.description).not.toContain('update_plan_verification');
            expect(tool?.description).toContain("skipped remains available only where skip is actually allowed by the backend");
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                status: 'verified',
                convergence_reason: 'zero_blocking',
                round: 1,
                generation: 3,
            });
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('gaps');
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('required_delegates');
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('created_after');
            expect((tool?.inputSchema).examples).toHaveLength(1);
            expect((tool?.inputSchema).properties.status.enum).not.toContain('reviewing');
            expect(tool?.inputSchema.properties).not.toHaveProperty('session_id');
            expect(tool?.inputSchema.properties).not.toHaveProperty('gaps');
            expect(tool?.inputSchema.properties).not.toHaveProperty('required_delegates');
            expect(tool?.inputSchema.properties).not.toHaveProperty('created_after');
            expect(tool?.inputSchema.properties).not.toHaveProperty('rescue_budget_exhausted');
            expect(tool?.inputSchema.properties).not.toHaveProperty('max_wait_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('poll_interval_ms');
            expect(tool?.inputSchema.required).toEqual(['status', 'generation']);
        });
    });
    describe('run_verification_enrichment', () => {
        const tool = PLAN_TOOLS.find((t) => t.name === 'run_verification_enrichment');
        it('should expose the backend-owned enrichment helper', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('one-time verification enrichment helper');
            expect(tool?.description).toContain('do not pass session_id');
            expect(tool?.description).toContain('verifier chooses which enrichment specialists to run');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                selected_specialists: ['intent', 'code-quality'],
            });
            expect(tool?.inputSchema.properties).not.toHaveProperty('session_id');
            expect(tool?.inputSchema.properties).toHaveProperty('selected_specialists');
            expect(tool?.inputSchema.properties).not.toHaveProperty('disabled_specialists');
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('max_wait_ms');
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('poll_interval_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('max_wait_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('poll_interval_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('include_full_content');
            expect(tool?.inputSchema.properties).not.toHaveProperty('include_messages');
            expect(tool?.inputSchema.required).toEqual([]);
        });
    });
    describe('run_verification_round', () => {
        const tool = PLAN_TOOLS.find((t) => t.name === 'run_verification_round');
        it('should expose the backend-owned round driver', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('verification round driver');
            expect(tool?.description).toContain('do not pass session_id');
            expect(tool?.description).toContain('structured required critic findings');
            expect(tool?.description).toContain('verifier chooses which optional specialists to run');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                round: 2,
                selected_specialists: ['ux', 'pipeline-safety'],
            });
            expect(tool?.inputSchema.properties).not.toHaveProperty('session_id');
            expect(tool?.inputSchema.properties).toHaveProperty('selected_specialists');
            expect(tool?.inputSchema.properties).not.toHaveProperty('disabled_specialists');
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('max_wait_ms');
            expect((tool?.inputSchema).examples?.[0]).not.toHaveProperty('optional_wait_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('max_wait_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('optional_wait_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('poll_interval_ms');
            expect(tool?.inputSchema.properties).not.toHaveProperty('include_full_content');
            expect(tool?.inputSchema.properties).not.toHaveProperty('include_messages');
            expect(tool?.inputSchema.required).toEqual(['round']);
        });
    });
    describe('get_child_session_status', () => {
        const tool = allTools.find((t) => t.name === 'get_child_session_status');
        it('should document verifier debugging guidance and example payload', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('include_recent_messages=true');
            expect(tool?.description).toContain('last assistant/tool outputs');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'verification-child-session-id',
                include_recent_messages: true,
                message_limit: 10,
            });
        });
    });
    describe('send_ideation_session_message', () => {
        const tool = allTools.find((t) => t.name === 'send_ideation_session_message');
        it('should document full-context verifier nudges and example payload', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('repeat the full invariant context');
            expect(tool?.description).toContain('SESSION_ID, ROUND, expected critic/schema');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'verification-child-session-id',
            });
            expect((tool?.inputSchema).examples?.[0]?.message).toContain('SESSION_ID');
            expect((tool?.inputSchema).examples?.[0]?.message).toContain('ROUND: 2');
            expect((tool?.inputSchema).examples?.[0]?.message).toContain('publish_verification_finding');
        });
    });
    describe('get_team_session_state', () => {
        const tool = allTools.find((t) => t.name === 'get_team_session_state');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('session_id');
            expect(tool?.inputSchema.required).toContain('session_id');
        });
    });
    describe('save_team_session_state', () => {
        const tool = allTools.find((t) => t.name === 'save_team_session_state');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('session_id');
            expect(tool?.inputSchema.properties).toHaveProperty('team_composition');
            expect(tool?.inputSchema.properties).toHaveProperty('phase');
            expect(tool?.inputSchema.required).toContain('session_id');
            expect(tool?.inputSchema.required).toContain('team_composition');
            expect(tool?.inputSchema.required).toContain('phase');
        });
        it('should have team_composition array with correct item schema', () => {
            const teamComp = tool?.inputSchema.properties?.team_composition;
            expect(teamComp).toBeDefined();
            expect(teamComp.type).toBe('array');
            expect(teamComp.items).toBeDefined();
            expect(teamComp.items.type).toBe('object');
            expect(teamComp.items.properties).toHaveProperty('name');
            expect(teamComp.items.properties).toHaveProperty('role');
            expect(teamComp.items.properties).toHaveProperty('prompt');
            expect(teamComp.items.properties).toHaveProperty('model');
            expect(teamComp.items.required).toContain('name');
            expect(teamComp.items.required).toContain('role');
            expect(teamComp.items.required).toContain('prompt');
            expect(teamComp.items.required).toContain('model');
        });
    });
});
describe('Tool allowlist for new agent types', () => {
    it('ralphx-ideation-team-lead should have all team coordination tools', () => {
        const allowlist = toolsByAgent()[IDEATION_TEAM_LEAD];
        expect(allowlist).toContain('request_team_plan');
        expect(allowlist).toContain('request_teammate_spawn');
        expect(allowlist).toContain('create_team_artifact');
        expect(allowlist).toContain('get_team_artifacts');
        expect(allowlist).toContain('get_team_session_state');
        expect(allowlist).toContain('save_team_session_state');
    });
    it('ideation-team-member should have limited team tools', () => {
        const allowlist = toolsByAgent()[IDEATION_TEAM_MEMBER];
        // Should have artifact tools
        expect(allowlist).toContain('create_team_artifact');
        expect(allowlist).toContain('get_team_artifacts');
        // Should NOT have lead-only tools
        expect(allowlist).not.toContain('request_team_plan');
        expect(allowlist).not.toContain('request_teammate_spawn');
        expect(allowlist).not.toContain('save_team_session_state');
    });
    it('worker-team-member should have artifact tools', () => {
        const allowlist = toolsByAgent()[WORKER_TEAM_MEMBER];
        // Should have artifact tools (document decisions)
        expect(allowlist).toContain('create_team_artifact');
        expect(allowlist).toContain('get_team_artifacts');
        // Should NOT have lead-only tools
        expect(allowlist).not.toContain('request_team_plan');
        expect(allowlist).not.toContain('request_teammate_spawn');
        expect(allowlist).not.toContain('save_team_session_state');
    });
});
// ===========================================================================
// TDD tests for --allowed-tools CLI arg parsing (Wave 1)
// These tests FAIL until Wave 2 implementation is complete.
// ===========================================================================
describe('parseAllowedToolsFromArgs', () => {
    let originalArgv;
    beforeEach(() => {
        originalArgv = [...process.argv];
        // Start clean — no --allowed-tools arg
        process.argv = process.argv.filter((a) => !a.startsWith('--allowed-tools'));
    });
    afterEach(() => {
        process.argv = originalArgv;
    });
    it('returns ["tool1", "tool2"] when --allowed-tools=tool1,tool2', () => {
        process.argv = [...process.argv, '--allowed-tools=tool1,tool2'];
        const result = parseAllowedToolsFromArgs();
        expect(result).toEqual(['tool1', 'tool2']);
    });
    it('returns [] when --allowed-tools=__NONE__ (explicit empty sentinel)', () => {
        process.argv = [...process.argv, '--allowed-tools=__NONE__'];
        const result = parseAllowedToolsFromArgs();
        expect(result).toEqual([]);
    });
    it('returns undefined when --allowed-tools= (empty value falls through)', () => {
        process.argv = [...process.argv, '--allowed-tools='];
        const result = parseAllowedToolsFromArgs();
        expect(result).toBeUndefined();
    });
    it('returns undefined when --allowed-tools is absent', () => {
        // argv already cleaned in beforeEach
        const result = parseAllowedToolsFromArgs();
        expect(result).toBeUndefined();
    });
    it('skips invalid tool names (uppercase, spaces) and emits warning', () => {
        const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => { });
        process.argv = [
            ...process.argv,
            '--allowed-tools=valid_tool,INVALID_UPPER,has space',
        ];
        const result = parseAllowedToolsFromArgs();
        expect(result).toContain('valid_tool');
        expect(result).not.toContain('INVALID_UPPER');
        expect(result).not.toContain('has space');
        expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('INVALID_UPPER'));
        consoleSpy.mockRestore();
    });
    it('includes unknown tool names (not in ALL_TOOLS) and emits warning', () => {
        const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => { });
        process.argv = [
            ...process.argv,
            '--allowed-tools=get_session_plan,xyz_not_in_registry',
        ];
        const result = parseAllowedToolsFromArgs();
        expect(result).toContain('get_session_plan');
        expect(result).toContain('xyz_not_in_registry'); // included, NOT dropped
        expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('xyz_not_in_registry'));
        consoleSpy.mockRestore();
    });
});
describe('getAllowedToolNames - CLI arg priority chain', () => {
    let originalArgv;
    beforeEach(() => {
        originalArgv = [...process.argv];
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
        process.argv = process.argv.filter((a) => !a.startsWith('--allowed-tools'));
    });
    afterEach(() => {
        process.argv = originalArgv;
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    it('uses --allowed-tools CLI arg when RALPHX_ALLOWED_MCP_TOOLS env var is not set', () => {
        process.argv = [...process.argv, '--allowed-tools=get_session_plan,create_team_artifact'];
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan', 'create_team_artifact']);
    });
    it('env var takes priority over --allowed-tools CLI arg', () => {
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan';
        process.argv = [...process.argv, '--allowed-tools=create_team_artifact'];
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan']); // env var wins
        expect(tools).not.toContain('create_team_artifact');
    });
    it('--allowed-tools takes priority over legacy fallback resolution', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        process.argv = [...process.argv, '--allowed-tools=get_session_plan'];
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan']);
        expect(tools).not.toEqual(toolsByAgent()[IDEATION_TEAM_LEAD]);
    });
    it('legacy TOOL_ALLOWLIST fallback emits deprecation warning when canonical metadata is absent', () => {
        const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => { });
        const originalTools = toolsByAgent()['legacy-fallback-agent'];
        setLegacyToolAllowlistEntryForTest('legacy-fallback-agent', ['get_session_plan']);
        try {
            setAgentType('legacy-fallback-agent');
            const tools = getAllowedToolNames();
            expect(tools).toEqual(['get_session_plan']);
            expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('fallback TOOL_ALLOWLIST (legacy only)'));
        }
        finally {
            setLegacyToolAllowlistEntryForTest('legacy-fallback-agent', originalTools);
            consoleSpy.mockRestore();
        }
    });
});
// ===========================================================================
// delete_task_proposal MCP tool — alias for archive_task_proposal
// ===========================================================================
describe('delete_task_proposal tool', () => {
    const allTools = getAllTools();
    const tool = allTools.find((t) => t.name === 'delete_task_proposal');
    it('should exist in ALL_TOOLS', () => {
        expect(tool).toBeDefined();
    });
    it('should have correct inputSchema with required proposal_id field', () => {
        expect(tool?.inputSchema).toBeDefined();
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('proposal_id');
        expect(tool?.inputSchema.required).toContain('proposal_id');
    });
    it('should be in TOOL_ALLOWLIST for ralphx-ideation', () => {
        expect(toolsByAgent()[ORCHESTRATOR_IDEATION]).toContain('delete_task_proposal');
    });
    it('should be in TOOL_ALLOWLIST for ralphx-ideation-team-lead', () => {
        expect(toolsByAgent()[IDEATION_TEAM_LEAD]).toContain('delete_task_proposal');
    });
    it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
        expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('delete_task_proposal');
    });
    it('should be returned by getFilteredTools for ralphx-ideation', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const toolNames = getFilteredTools().map((t) => t.name);
        expect(toolNames).toContain('delete_task_proposal');
    });
    it('should be returned by getFilteredTools for ralphx-ideation-team-lead', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        const toolNames = getFilteredTools().map((t) => t.name);
        expect(toolNames).toContain('delete_task_proposal');
    });
});
// ===========================================================================
// revert_and_skip MCP tool — tool definition, allowlist, and dispatch coverage
// ===========================================================================
describe('revert_and_skip tool', () => {
    const allTools = getAllTools();
    const tool = allTools.find((t) => t.name === 'revert_and_skip');
    it('should exist in ALL_TOOLS', () => {
        expect(tool).toBeDefined();
    });
    it('should have correct inputSchema with required fields', () => {
        expect(tool?.inputSchema).toBeDefined();
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('session_id');
        expect(tool?.inputSchema.properties).toHaveProperty('plan_version_to_restore');
        expect(tool?.inputSchema.required).toContain('session_id');
        expect(tool?.inputSchema.required).toContain('plan_version_to_restore');
    });
    it('should be in TOOL_ALLOWLIST for ralphx-ideation', () => {
        expect(toolsByAgent()[ORCHESTRATOR_IDEATION]).toContain('revert_and_skip');
    });
    it('should be in TOOL_ALLOWLIST for ralphx-ideation-team-lead', () => {
        expect(toolsByAgent()[IDEATION_TEAM_LEAD]).toContain('revert_and_skip');
    });
    it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
        expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('revert_and_skip');
    });
    it('should be returned by getFilteredTools for ralphx-ideation', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const toolNames = getFilteredTools().map((t) => t.name);
        expect(toolNames).toContain('revert_and_skip');
    });
    it('should be returned by getFilteredTools for ralphx-ideation-team-lead', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        const toolNames = getFilteredTools().map((t) => t.name);
        expect(toolNames).toContain('revert_and_skip');
    });
});
// ===========================================================================
// get_acceptance_status + get_pending_confirmations tool definitions + allowlist
// ===========================================================================
describe('acceptance gate tools', () => {
    const allTools = getAllTools();
    describe('get_acceptance_status', () => {
        const tool = allTools.find((t) => t.name === 'get_acceptance_status');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have correct inputSchema with required session_id', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.properties).toHaveProperty('session_id');
            expect(tool?.inputSchema.required).toContain('session_id');
        });
        it('should be in TOOL_ALLOWLIST for ralphx-ideation', () => {
            expect(toolsByAgent()[ORCHESTRATOR_IDEATION]).toContain('get_acceptance_status');
        });
        it('should be in TOOL_ALLOWLIST for ralphx-ideation-team-lead', () => {
            expect(toolsByAgent()[IDEATION_TEAM_LEAD]).toContain('get_acceptance_status');
        });
        it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
            expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('get_acceptance_status');
        });
        it('should be returned by getFilteredTools for ralphx-ideation', () => {
            setAgentType(ORCHESTRATOR_IDEATION);
            const toolNames = getFilteredTools().map((t) => t.name);
            expect(toolNames).toContain('get_acceptance_status');
        });
        it('should be returned by getFilteredTools for ralphx-ideation-team-lead', () => {
            setAgentType(IDEATION_TEAM_LEAD);
            const toolNames = getFilteredTools().map((t) => t.name);
            expect(toolNames).toContain('get_acceptance_status');
        });
    });
    describe('get_pending_confirmations', () => {
        const tool = allTools.find((t) => t.name === 'get_pending_confirmations');
        it('should exist in ALL_TOOLS', () => {
            expect(tool).toBeDefined();
        });
        it('should have an object inputSchema with no required fields', () => {
            expect(tool?.inputSchema).toBeDefined();
            expect(tool?.inputSchema.type).toBe('object');
            expect(tool?.inputSchema.required).toEqual([]);
        });
        it('should be in TOOL_ALLOWLIST for ralphx-ideation', () => {
            expect(toolsByAgent()[ORCHESTRATOR_IDEATION]).toContain('get_pending_confirmations');
        });
        it('should be in TOOL_ALLOWLIST for ralphx-ideation-team-lead', () => {
            expect(toolsByAgent()[IDEATION_TEAM_LEAD]).toContain('get_pending_confirmations');
        });
        it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
            expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('get_pending_confirmations');
        });
        it('should be returned by getFilteredTools for ralphx-ideation', () => {
            setAgentType(ORCHESTRATOR_IDEATION);
            const toolNames = getFilteredTools().map((t) => t.name);
            expect(toolNames).toContain('get_pending_confirmations');
        });
        it('should be returned by getFilteredTools for ralphx-ideation-team-lead', () => {
            setAgentType(IDEATION_TEAM_LEAD);
            const toolNames = getFilteredTools().map((t) => t.name);
            expect(toolNames).toContain('get_pending_confirmations');
        });
    });
});
describe('agent workspace repair tool', () => {
    const allTools = getAllTools();
    const tool = allTools.find((t) => t.name === 'complete_agent_workspace_repair');
    it('should exist in ALL_TOOLS', () => {
        expect(tool).toBeDefined();
    });
    it('should require repair verification fields', () => {
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('conversation_id');
        expect(tool?.inputSchema.properties).toHaveProperty('repair_commit_sha');
        expect(tool?.inputSchema.properties).toHaveProperty('resolved_base_ref');
        expect(tool?.inputSchema.properties).toHaveProperty('resolved_base_commit');
        expect(tool?.inputSchema.properties).toHaveProperty('summary');
        expect(tool?.inputSchema.required).toEqual(expect.arrayContaining([
            'conversation_id',
            'repair_commit_sha',
            'resolved_base_ref',
            'resolved_base_commit',
            'summary',
        ]));
    });
});
// ===========================================================================
// RalphX native delegation bridge tools
// ===========================================================================
describe('delegation bridge tools', () => {
    const allTools = getAllTools();
    it.each(['delegate_start', 'delegate_wait', 'delegate_cancel'])('%s should exist in ALL_TOOLS', (toolName) => {
        expect(allTools.find((tool) => tool.name === toolName)).toBeDefined();
    });
    it('delegate_start should expose optional parent_session_id plus required agent_name and prompt', () => {
        const tool = allTools.find((entry) => entry.name === 'delegate_start');
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_session_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_turn_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_message_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_conversation_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_tool_use_id');
        expect(tool?.inputSchema.properties).toHaveProperty('delegated_session_id');
        expect(tool?.inputSchema.required).toEqual(expect.arrayContaining(['agent_name', 'prompt']));
        expect(tool?.inputSchema.required).not.toContain('parent_session_id');
    });
    it('delegate_wait should support delegated-status hydration options', () => {
        const tool = allTools.find((entry) => entry.name === 'delegate_wait');
        expect(tool?.inputSchema.properties).toHaveProperty('include_delegated_status');
        expect(tool?.inputSchema.properties).toHaveProperty('include_child_status');
        expect(tool?.inputSchema.properties).toHaveProperty('include_messages');
        expect(tool?.inputSchema.properties).toHaveProperty('message_limit');
    });
    it.each([ORCHESTRATOR_IDEATION, ORCHESTRATOR_IDEATION_READONLY])('%s should expose delegation bridge tools', (agent) => {
        expect(toolsByAgent()[agent]).toContain('delegate_start');
        expect(toolsByAgent()[agent]).toContain('delegate_wait');
        expect(toolsByAgent()[agent]).toContain('delegate_cancel');
    });
    it.each([WORKER, REVIEWER, MERGER])('%s should expose delegation bridge tools in the fallback allowlist', (agent) => {
        expect(toolsByAgent()[agent]).toContain('delegate_start');
        expect(toolsByAgent()[agent]).toContain('delegate_wait');
        expect(toolsByAgent()[agent]).toContain('delegate_cancel');
    });
    it.each([ORCHESTRATOR_IDEATION, ORCHESTRATOR_IDEATION_READONLY, WORKER, REVIEWER, MERGER])('%s should return delegate_start from getFilteredTools', (agent) => {
        setAgentType(agent);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('delegate_start');
    });
    it('ideation team lead should not receive delegation bridge tools from getFilteredTools', () => {
        setAgentType(IDEATION_TEAM_LEAD);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).not.toContain('delegate_start');
        expect(toolNames).not.toContain('delegate_wait');
        expect(toolNames).not.toContain('delegate_cancel');
    });
});
describe('verification round helper tools', () => {
    const allTools = getAllTools();
    it('assess_verification_round should not exist in ALL_TOOLS', () => {
        const tool = allTools.find((entry) => entry.name === 'assess_verification_round');
        expect(tool).toBeUndefined();
    });
    it('await_verification_round_settlement should not exist in ALL_TOOLS', () => {
        const tool = allTools.find((entry) => entry.name === 'await_verification_round_settlement');
        expect(tool).toBeUndefined();
    });
    it('plan verifier should only expose the high-level verification helpers', () => {
        expect(toolsByAgent()[PLAN_VERIFIER]).toContain('run_verification_enrichment');
        expect(toolsByAgent()[PLAN_VERIFIER]).toContain('run_verification_round');
        setAgentType(PLAN_VERIFIER);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('run_verification_enrichment');
        expect(toolNames).toContain('run_verification_round');
        expect(toolNames).toContain('complete_plan_verification');
        expect(toolNames).not.toContain('assess_verification_round');
        expect(toolNames).not.toContain('run_required_verification_critic_round');
        expect(toolNames).not.toContain('await_verification_round_settlement');
        expect(toolNames).not.toContain('delegate_start');
    });
});
// ===========================================================================
// Specialist / Critic / Advocate canonical allowlist assertions + YAML parity
// ===========================================================================
describe('canonical specialist allowlist entries', () => {
    it('keeps every current resolved allowlist entry backed by canonical metadata', () => {
        for (const agent of Object.keys(toolsByAgent()).filter((agent) => agent !== 'debug')) {
            expect(loadCanonicalMcpTools(agent)).toBeDefined();
        }
    });
    const artifactSpecialists = [
        IDEATION_SPECIALIST_BACKEND,
        IDEATION_SPECIALIST_FRONTEND,
        IDEATION_SPECIALIST_INFRA,
    ];
    const verificationFindingSpecialists = [
        IDEATION_SPECIALIST_CODE_QUALITY,
        IDEATION_SPECIALIST_UX,
        IDEATION_SPECIALIST_PROMPT_QUALITY,
        IDEATION_SPECIALIST_INTENT,
        IDEATION_SPECIALIST_PIPELINE_SAFETY,
        IDEATION_SPECIALIST_STATE_MACHINE,
    ];
    const parentContextSpecialists = [
        IDEATION_SPECIALIST_BACKEND,
        IDEATION_SPECIALIST_FRONTEND,
        IDEATION_SPECIALIST_INFRA,
    ];
    it.each(artifactSpecialists)('%s should include create_team_artifact', (agent) => {
        expect(toolsByAgent()[agent]).toContain('create_team_artifact');
    });
    it.each(artifactSpecialists)('%s should include get_team_artifacts', (agent) => {
        expect(toolsByAgent()[agent]).toContain('get_team_artifacts');
    });
    it.each(verificationFindingSpecialists)('%s should include publish_verification_finding', (agent) => {
        expect(toolsByAgent()[agent]).toContain('publish_verification_finding');
    });
    it.each(verificationFindingSpecialists)('%s should not include create_team_artifact', (agent) => {
        expect(toolsByAgent()[agent]).not.toContain('create_team_artifact');
    });
    it.each(verificationFindingSpecialists)('%s should not include get_team_artifacts', (agent) => {
        expect(toolsByAgent()[agent]).not.toContain('get_team_artifacts');
    });
    it.each(parentContextSpecialists)('%s should include get_parent_session_context', (agent) => {
        expect(toolsByAgent()[agent]).toContain('get_parent_session_context');
    });
    it('IDEATION_TEAM_MEMBER should include get_parent_session_context', () => {
        expect(toolsByAgent()[IDEATION_TEAM_MEMBER]).toContain('get_parent_session_context');
    });
    it.each([
        IDEATION_TEAM_MEMBER,
        WORKER_TEAM_LEAD,
        WORKER_TEAM_MEMBER,
    ])('%s should stay aligned with canonical mcp_tools', (agent) => {
        expect(loadCanonicalMcpTools(agent)).toEqual(toolsByAgent()[agent]);
    });
    it('IDEATION_CRITIC should include create_team_artifact', () => {
        expect(toolsByAgent()[IDEATION_CRITIC]).toContain('create_team_artifact');
    });
    it('IDEATION_CRITIC should include get_team_artifacts', () => {
        expect(toolsByAgent()[IDEATION_CRITIC]).toContain('get_team_artifacts');
    });
    it('IDEATION_ADVOCATE should include create_team_artifact', () => {
        expect(toolsByAgent()[IDEATION_ADVOCATE]).toContain('create_team_artifact');
    });
    it('IDEATION_ADVOCATE should include get_team_artifacts', () => {
        expect(toolsByAgent()[IDEATION_ADVOCATE]).toContain('get_team_artifacts');
    });
    it.each([
        PLAN_CRITIC_COMPLETENESS,
        PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
    ])('%s should include publish_verification_finding', (agent) => {
        expect(toolsByAgent()[agent]).toContain('publish_verification_finding');
    });
    it.each([
        PLAN_CRITIC_COMPLETENESS,
        PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
    ])('%s should not include create_team_artifact', (agent) => {
        expect(toolsByAgent()[agent]).not.toContain('create_team_artifact');
    });
    it.each([
        PLAN_CRITIC_COMPLETENESS,
        PLAN_CRITIC_IMPLEMENTATION_FEASIBILITY,
    ])('%s should stay bounded to direct read tools', (agent) => {
        expect(toolsByAgent()[agent]).not.toContain('get_team_artifacts');
    });
});
//# sourceMappingURL=tools.test.js.map