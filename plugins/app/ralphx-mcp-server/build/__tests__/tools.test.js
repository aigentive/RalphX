/**
 * Unit tests for MCP tool definitions and authorization logic
 * Tests agent team coordination features
 */
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { getAllowedToolNames, getFilteredTools, getToolsByAgent, isToolAllowed, setAgentType, getAllTools, getToolRecoveryHint, formatToolErrorMessage, parseAllowedToolsFromArgs, } from '../tools.js';
import { canonicalAgentName, loadCanonicalMcpTools } from '../canonical-agent-metadata.js';
import { setLegacyToolAllowlistEntryForTest } from '../tool-authorization.js';
import { PLAN_TOOLS } from '../plan-tools.js';
import { buildAppendTaskToIdeationPlanPayload } from '../append-task-payload.js';
import { AGENT_WORKSPACE_TOOLS, callAgentWorkspaceTool, callCheckAgentWorkspacePublishReadinessTool, callCompleteAgentWorkspacePrFixTool, callCompleteWorkspaceReviewRunTool, callCompletePrReviewRunTool, callCompleteAgentWorkspaceRepairTool, callGetAgentWorkspacePrFixContextTool, callGetPrReviewContextTool, callGetWorkspaceReviewContextTool, callGetWorkspaceReviewDiffPageTool, callListWorkspaceReviewFilesTool, callGetAgentWorkspacePublishStatusTool, callPublishAgentWorkspaceTool, callProposePrReviewActionTool, callReadAgentWorkspacePrCommentTool, callSubmitAgentWorkspacePrDescriptionTool, callUpdateAgentWorkspaceFromBaseTool, callWriteWorkspaceReviewArtifactTool, callWriteWorkspaceReviewHunkAnnotationsTool, callWritePrReviewArtifactTool, isAgentWorkspaceToolName, } from '../agent-workspace-tools.js';
import { ORCHESTRATOR_IDEATION, ORCHESTRATOR_IDEATION_READONLY, IDEATION_SPECIALIST_BACKEND, IDEATION_SPECIALIST_FRONTEND, IDEATION_SPECIALIST_INFRA, IDEATION_CRITIC, IDEATION_ADVOCATE, REVIEWER, GENERAL_EXPLORER, GENERAL_WORKER, PR_REVIEWER, AGENT_WORKSPACE_REPAIR, AGENT_WORKSPACE_PR_FIXER, PLAN_COMPLEXITY_ASSESSOR, WORKSPACE_REVIEWER, AUTOMATION_SETUP, WORKER, MERGER, CHAT_PROJECT, } from '../agentNames.js';
function toolsByAgent() {
    return getToolsByAgent();
}
function inputSchemaProperties(toolName) {
    const tool = getAllTools().find((candidate) => candidate.name === toolName);
    expect(tool, `${toolName} tool`).toBeDefined();
    return (tool.inputSchema.properties ?? {});
}
describe('getAllowedToolNames', () => {
    beforeEach(() => {
        // Clear env var before each test
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
        delete process.env.RALPHX_AGENT_PROFILE;
        delete process.env.RALPHX_COORDINATION_MODE;
    });
    afterEach(() => {
        // Clean up env var after each test
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
        delete process.env.RALPHX_AGENT_PROFILE;
        delete process.env.RALPHX_COORDINATION_MODE;
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
        setAgentType(ORCHESTRATOR_IDEATION);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan';
        const tools = getAllowedToolNames();
        // Should return env var list, not agent type allowlist
        expect(tools).toEqual(['get_session_plan']);
        expect(tools).not.toEqual(toolsByAgent()[ORCHESTRATOR_IDEATION]);
    });
    it('keeps delegation tools for the native ideation orchestrator', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'delegate_start,get_session_plan,delegate_wait';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['delegate_start', 'get_session_plan', 'delegate_wait']);
    });
    it('prefers canonical mcp_tools when available', () => {
        setAgentType('qa-prep');
        const tools = getAllowedToolNames();
        expect(tools).toEqual(loadCanonicalMcpTools('qa-prep'));
        expect(tools).toContain('fs_read_file');
        expect(tools).toContain('fs_grep');
    });
    it('grants the canonical PersonaBuilder surface only to the persona extractor', () => {
        const extractorTools = [
            'fs_read_file',
            'fs_list_dir',
            'fs_grep',
            'fs_glob',
            'ask_user_question',
            'save_persona_draft',
            'get_persona_draft',
        ];
        setAgentType('ralphx-persona-extractor');
        expect(getAllowedToolNames()).toEqual(extractorTools);
        expect(isToolAllowed('save_persona_draft')).toBe(true);
        expect(isToolAllowed('get_persona_draft')).toBe(true);
        setAgentType(GENERAL_EXPLORER);
        expect(isToolAllowed('save_persona_draft')).toBe(false);
        expect(isToolAllowed('get_persona_draft')).toBe(false);
    });
    it('grants workflow tools only inside Workflow conversations', () => {
        setAgentType(GENERAL_WORKER);
        expect(getAllowedToolNames()).not.toContain('create_agent_workflow_script');
        expect(getAllowedToolNames()).not.toContain('start_agent_workflow_run');
        process.env.RALPHX_COORDINATION_MODE = 'rx_native_workflow';
        const workflowTools = getAllowedToolNames();
        expect(workflowTools).toContain('create_agent_workflow_script');
        expect(workflowTools).toContain('start_agent_workflow_run');
        expect(workflowTools).toContain('get_agent_workflow_run');
        process.env.RALPHX_COORDINATION_MODE = 'codex_native_ultra';
        expect(getAllowedToolNames()).not.toContain('create_agent_workflow_script');
    });
    it('uses profile-specific canonical mcp_tools when RALPHX_AGENT_PROFILE is set', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        process.env.RALPHX_AGENT_PROFILE = 'plan';
        const tools = getAllowedToolNames();
        expect(tools).toEqual(loadCanonicalMcpTools(ORCHESTRATOR_IDEATION, 'plan'));
        expect(tools).toContain('delegate_start');
        expect(tools).toContain('get_conversation_transcript');
        expect(tools).not.toContain('create_task_proposal');
        expect(tools).not.toContain('finalize_proposals');
    });
    it('grants coordinator and member Team tools only to their canonical roles in RX-native Team mode', () => {
        process.env.RALPHX_COORDINATION_MODE = 'rx_native_team';
        setAgentType(GENERAL_WORKER);
        expect(getAllowedToolNames()).not.toContain('team_assign');
        expect(getAllowedToolNames()).toEqual(expect.arrayContaining(['team_send_message', 'team_roster']));
        setAgentType(GENERAL_EXPLORER);
        expect(getAllowedToolNames()).toEqual(expect.arrayContaining(['team_send_message', 'team_roster']));
        expect(getAllowedToolNames()).not.toContain('team_add_member');
        setAgentType('ralphx-chat-task');
        expect(getAllowedToolNames()).not.toContain('team_send_message');
        process.env.RALPHX_AGENT_PROFILE = 'team_coordinator';
        expect(getAllowedToolNames()).toEqual(expect.arrayContaining([
            'team_add_member',
            'team_assign',
            'team_list',
            'team_stop_member',
            'team_send_message',
        ]));
        setAgentType(GENERAL_WORKER);
        expect(getAllowedToolNames()).toEqual(expect.arrayContaining([
            'team_add_member',
            'team_assign',
            'team_list',
            'team_stop_member',
            'team_send_message',
        ]));
        process.env.RALPHX_COORDINATION_MODE = 'rx_native_workflow';
        expect(getAllowedToolNames()).not.toContain('team_assign');
    });
    it('rejects canonical agent path traversal attempts', () => {
        expect(loadCanonicalMcpTools('../secrets')).toBeUndefined();
        expect(loadCanonicalMcpTools(ORCHESTRATOR_IDEATION, '../secrets')).toBeUndefined();
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
    it('resolves the plan complexity legacy alias to canonical metadata', () => {
        setAgentType('plan-complexity');
        expect(canonicalAgentName('plan-complexity')).toBe('ralphx-utility-plan-complexity');
        expect(getAllowedToolNames()).toEqual(loadCanonicalMcpTools('plan-complexity'));
        expect(getAllowedToolNames()).toEqual(['submit_plan_complexity_assessment']);
    });
});
describe('getToolRecoveryHint', () => {
    it('does not expose recovery guidance for removed update_plan_verification', () => {
        expect(getToolRecoveryHint('update_plan_verification')).toBeNull();
    });
    it('documents caller-derived exact-proof completion', () => {
        const hint = getToolRecoveryHint('complete_plan_verification');
        expect(hint).toContain('Pass an empty object');
        expect(hint).toContain('exact current artifact');
        expect(hint).toContain('failed, cancelled, or mismatched run');
    });
    it('documents the simplified verification status read', () => {
        const hint = getToolRecoveryHint('get_plan_verification');
        expect(hint).toContain('visible Verify Plan action status');
        expect(hint).toContain('Pass session_id outside an ideation runtime');
    });
    it('returns child-debugging guidance for get_child_session_status', () => {
        const hint = getToolRecoveryHint('get_child_session_status');
        expect(hint).toContain('include_recent_messages=true');
        expect(hint).toContain('Example payload:');
    });
    it('returns full-context guidance for send_ideation_session_message', () => {
        const hint = getToolRecoveryHint('send_ideation_session_message');
        expect(hint).toContain('full task context');
        expect(hint).toContain('Example payload:');
    });
    it('returns cleanup guidance for agent task ledger progression tools', () => {
        for (const toolName of ['claim_agent_task', 'complete_agent_task', 'update_agent_task']) {
            const hint = getToolRecoveryHint(toolName);
            expect(hint).toContain('one meaningful task cannot be claimed, activated, or completed');
            expect(hint).toContain('state=dropped');
            expect(hint).toContain('create multiple concrete tasks');
        }
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
    it('appends details and a usage hint for model-native verification completion', () => {
        const text = formatToolErrorMessage('complete_plan_verification', 'Verification proof was rejected.', 'The current action no longer owns the artifact.');
        expect(text).toContain('ERROR: Verification proof was rejected.');
        expect(text).toContain('Details: The current action no longer owns the artifact.');
        expect(text).toContain('Usage hint for complete_plan_verification:');
        expect(text).toContain('Pass an empty object');
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
    it('advertises create_task_proposal dependency fields supported by the backend', () => {
        const tool = getAllTools().find((candidate) => candidate.name === 'create_task_proposal');
        expect(tool).toBeDefined();
        const properties = inputSchemaProperties('create_task_proposal');
        expect(properties.depends_on).toMatchObject({
            type: 'array',
            items: { type: 'string' },
        });
        expect(tool.inputSchema.required ?? []).not.toContain('depends_on');
    });
    it('advertises update_task_proposal dependency edit fields supported by the backend', () => {
        const tool = getAllTools().find((candidate) => candidate.name === 'update_task_proposal');
        expect(tool).toBeDefined();
        const properties = inputSchemaProperties('update_task_proposal');
        expect(properties.add_depends_on).toMatchObject({
            type: 'array',
            items: { type: 'string' },
        });
        expect(properties.add_blocks).toMatchObject({
            type: 'array',
            items: { type: 'string' },
        });
        expect(tool.inputSchema.required ?? []).not.toContain('add_depends_on');
        expect(tool.inputSchema.required ?? []).not.toContain('add_blocks');
    });
    it('keeps run_task_validation focused on post-change evidence while baseline stays diagnostic', () => {
        const tool = getAllTools().find((candidate) => candidate.name === 'run_task_validation');
        expect(tool, 'run_task_validation tool').toBeDefined();
        expect(tool.description).toContain('authoritative post-change validation evidence');
        expect(tool.description).toContain('purpose=baseline');
        expect(tool.description).toContain('explicit diagnostics');
        expect(tool.description).not.toContain('baseline/final validation commands');
        const properties = inputSchemaProperties('run_task_validation');
        expect(properties.purpose.enum).toContain('baseline');
        expect(properties.purpose.description).toContain('Baseline is for explicit diagnostics');
        expect(properties.purpose.description).not.toContain('normal first step');
    });
});
describe('getFilteredTools', () => {
    beforeEach(() => {
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    afterEach(() => {
        delete process.env.RALPHX_ALLOWED_MCP_TOOLS;
    });
    it('should return the native ideation tool set without legacy team controls', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).not.toContain('create_team_artifact');
        expect(toolNames).not.toContain('get_team_artifacts');
        expect(toolNames).not.toContain('request_team_plan');
        expect(toolNames).not.toContain('request_teammate_spawn');
        expect(toolNames).not.toContain('get_team_session_state');
        expect(toolNames).not.toContain('save_team_session_state');
        // Should include ideation tools
        expect(toolNames).toContain('create_task_proposal');
        expect(toolNames).toContain('update_task_proposal');
        expect(toolNames).toContain('get_session_plan');
        expect(toolNames).not.toContain('update_plan_verification');
        // Should match allowlist count
        expect(tools.length).toBe(toolsByAgent()[ORCHESTRATOR_IDEATION].length);
    });
    it('should return only allowed tools for the backend ideation specialist', () => {
        setAgentType(IDEATION_SPECIALIST_BACKEND);
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
        expect(tools.length).toBe(toolsByAgent()[IDEATION_SPECIALIST_BACKEND].length);
    });
    it('should return the current worker tool set', () => {
        setAgentType(WORKER);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).not.toContain('create_team_artifact');
        expect(toolNames).not.toContain('get_team_artifacts');
        // Should include worker step tools
        expect(toolNames).toContain('start_step');
        expect(toolNames).toContain('complete_step');
        expect(toolNames).toContain('get_task_context');
        // Should NOT include lead-only tools
        expect(toolNames).not.toContain('request_team_plan');
        expect(toolNames).not.toContain('request_teammate_spawn');
        expect(toolNames).not.toContain('save_team_session_state');
        // Should match allowlist count
        expect(tools.length).toBe(toolsByAgent()[WORKER].length);
    });
    it('should expose only model-native verification tools to ralphx-ideation', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('get_plan_verification');
        expect(toolNames).toContain('complete_plan_verification');
        expect(toolNames).not.toContain('stop_verification');
        expect(toolNames).not.toContain('report_verification_round');
        expect(toolNames).not.toContain('run_verification_round');
    });
    it('should keep project chat on scoped parent-chat planning tools only', () => {
        setAgentType(CHAT_PROJECT);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toContain('suggest_task');
        expect(toolNames).toContain('list_tasks');
        expect(toolNames).toContain('propose_plan_mode');
        expect(toolNames).toContain('append_task_to_ideation_plan');
        expect(toolNames).toContain('create_followup_agent_conversation');
        expect(toolNames).toContain('register_agent_issue');
        expect(toolNames).not.toContain('start_ideation_session');
        expect(toolNames).not.toContain('create_child_session');
        expect(toolNames).not.toContain('create_followup_session');
        expect(toolNames).not.toContain('create_task_proposal');
        expect(toolNames).not.toContain('update_plan_artifact');
    });
    it.each([WORKER, REVIEWER])('%s should create visible follow-up Agent conversations', (agent) => {
        setAgentType(agent);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('create_followup_agent_conversation');
        expect(toolNames).toContain('register_agent_issue');
        expect(toolNames).not.toContain('create_followup_session');
    });
    it('register_agent_issue should expose issue and follow-up policy fields', () => {
        const properties = inputSchemaProperties('register_agent_issue');
        expect(properties).toHaveProperty('issue_kind');
        expect(properties).toHaveProperty('blocking_scope');
        expect(properties).toHaveProperty('auto_followup_eligible');
        expect(properties).toHaveProperty('followup_prompt');
        expect(properties).toHaveProperty('attach_to_issue_id');
        expect(properties).toHaveProperty('confirm_new');
        expect(properties).toHaveProperty('new_issue_reason');
        expect(properties).toHaveProperty('issue_check_token');
    });
    it('create_followup_agent_conversation should expose Agent conversation provenance fields', () => {
        const properties = inputSchemaProperties('create_followup_agent_conversation');
        expect(properties).toHaveProperty('origin_conversation_id');
        expect(properties).toHaveProperty('source_task_id');
        expect(properties).toHaveProperty('source_agent_name');
        expect(properties).toHaveProperty('blocker_fingerprint');
    });
    it('should let the general chat explorer propose a Plan-mode handoff without edit or ideation tools', () => {
        setAgentType(GENERAL_EXPLORER);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toContain('propose_plan_mode');
        expect(toolNames).toContain('delegate_start');
        expect(toolNames).toContain('delegate_wait');
        expect(toolNames).toContain('delegate_cancel');
        expect(toolNames).not.toContain('publish_agent_workspace');
        expect(toolNames).not.toContain('update_agent_workspace_from_base');
        expect(toolNames).not.toContain('start_ideation_session');
        expect(toolNames).not.toContain('create_child_session');
        expect(toolNames).not.toContain('create_task_proposal');
        expect(toolNames).not.toContain('update_plan_artifact');
    });
    it('should let the general edit worker propose a Plan-mode handoff without exposing ideation tools', () => {
        setAgentType(GENERAL_WORKER);
        const tools = getFilteredTools();
        const toolNames = tools.map((t) => t.name);
        expect(toolNames).toContain('propose_plan_mode');
        expect(toolNames).toContain('delegate_start');
        expect(toolNames).toContain('delegate_wait');
        expect(toolNames).toContain('delegate_cancel');
        expect(toolNames).not.toContain('start_ideation_session');
        expect(toolNames).not.toContain('create_child_session');
        expect(toolNames).not.toContain('create_task_proposal');
        expect(toolNames).not.toContain('update_plan_artifact');
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
        setAgentType(ORCHESTRATOR_IDEATION);
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
        setAgentType(ORCHESTRATOR_IDEATION);
        expect(isToolAllowed('create_task_proposal')).toBe(true);
    });
    it('should return false for disallowed tool', () => {
        setAgentType(IDEATION_SPECIALIST_BACKEND);
        expect(isToolAllowed('create_task_proposal')).toBe(false);
    });
    it('should return false for unknown tool', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        expect(isToolAllowed('nonexistent_tool')).toBe(false);
    });
    it('should respect env var override for allowed tool', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'get_session_plan';
        expect(isToolAllowed('get_session_plan')).toBe(true);
        expect(isToolAllowed('create_team_artifact')).toBe(false); // Not in env var
    });
    it('should respect env var override for disallowed tool', () => {
        setAgentType(IDEATION_SPECIALIST_BACKEND);
        process.env.RALPHX_ALLOWED_MCP_TOOLS = 'delete_task_proposal'; // Normally not allowed
        expect(isToolAllowed('delete_task_proposal')).toBe(true);
    });
});
describe('New team tool definitions', () => {
    const allTools = getAllTools();
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
        it('should expose only the general team-artifact contract', () => {
            expect(tool?.description).toContain('specialist findings');
            expect(tool?.description).not.toContain('verification child');
            expect(tool?.description).not.toContain('typed verification-finding');
            expect(tool?.inputSchema.properties?.session_id?.description).toContain('owns this team artifact');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'parent-session-id',
                artifact_type: 'TeamResearch',
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
        it('should document the general raw artifact lookup', () => {
            expect(tool?.description).toContain('raw artifact listing surface');
            expect(tool?.description).not.toContain('verification child');
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
    describe('get_plan_verification', () => {
        const tool = PLAN_TOOLS.find((candidate) => candidate.name === 'get_plan_verification');
        it('exposes a simplified optional session status read', () => {
            expect(tool).toBeDefined();
            expect(tool?.inputSchema.required).toEqual([]);
            expect(tool?.inputSchema.properties).toHaveProperty('session_id');
            expect(tool?.description).toContain('exact-artifact proof');
        });
    });
    describe('complete_plan_verification', () => {
        const tool = PLAN_TOOLS.find((candidate) => candidate.name === 'complete_plan_verification');
        it('exposes a zero-argument proof operation', () => {
            expect(tool).toBeDefined();
            expect(tool?.inputSchema.properties).toEqual({});
            expect(tool?.inputSchema.required).toEqual([]);
            expect(tool?.description).toContain('backend derives the run, conversation, planning session, and current artifact');
        });
    });
    describe('get_child_session_status', () => {
        const tool = allTools.find((t) => t.name === 'get_child_session_status');
        it('should document child debugging guidance and example payload', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('include_recent_messages=true');
            expect(tool?.description).toContain('last assistant/tool outputs');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'child-session-id',
                include_recent_messages: true,
                message_limit: 10,
            });
        });
    });
    describe('send_ideation_session_message', () => {
        const tool = allTools.find((t) => t.name === 'send_ideation_session_message');
        it('should document full-context nudges without retired verifier protocol', () => {
            expect(tool).toBeDefined();
            expect(tool?.description).toContain('full task context');
            expect(tool?.description).not.toContain('verification child');
            expect((tool?.inputSchema).examples?.[0]).toMatchObject({
                session_id: 'child-session-id',
            });
            expect((tool?.inputSchema).examples?.[0]?.message).toContain('team artifact');
            expect((tool?.inputSchema).examples?.[0]?.message).not.toContain('publish_verification_finding');
        });
    });
});
describe('Team artifact tool allowlists', () => {
    it('ralphx-ideation delegates artifact creation without legacy team coordination tools', () => {
        const allowlist = toolsByAgent()[ORCHESTRATOR_IDEATION];
        expect(allowlist).not.toContain('create_team_artifact');
        expect(allowlist).not.toContain('get_team_artifacts');
        expect(allowlist).not.toContain('request_team_plan');
        expect(allowlist).not.toContain('request_teammate_spawn');
        expect(allowlist).not.toContain('get_team_session_state');
        expect(allowlist).not.toContain('save_team_session_state');
    });
    it('ideation specialists should have limited artifact tools', () => {
        const allowlist = toolsByAgent()[IDEATION_SPECIALIST_BACKEND];
        // Should have artifact tools
        expect(allowlist).toContain('create_team_artifact');
        expect(allowlist).toContain('get_team_artifacts');
        // Should NOT have lead-only tools
        expect(allowlist).not.toContain('request_team_plan');
        expect(allowlist).not.toContain('request_teammate_spawn');
        expect(allowlist).not.toContain('save_team_session_state');
    });
    it('worker should not inherit specialist artifact tools', () => {
        const allowlist = toolsByAgent()[WORKER];
        expect(allowlist).not.toContain('create_team_artifact');
        expect(allowlist).not.toContain('get_team_artifacts');
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
        setAgentType(ORCHESTRATOR_IDEATION);
        process.argv = [...process.argv, '--allowed-tools=get_session_plan'];
        const tools = getAllowedToolNames();
        expect(tools).toEqual(['get_session_plan']);
        expect(tools).not.toEqual(toolsByAgent()[ORCHESTRATOR_IDEATION]);
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
    it('workspace reviewer allowlist mirrors canonical bounded Review artifact tools', () => {
        const tools = toolsByAgent()[WORKSPACE_REVIEWER];
        expect(tools).toEqual(loadCanonicalMcpTools(WORKSPACE_REVIEWER));
        expect(tools).toContain('fs_read_file');
        expect(tools).toContain('fs_list_dir');
        expect(tools).toContain('fs_grep');
        expect(tools).toContain('fs_glob');
        expect(tools).toContain('get_artifact');
        expect(tools).toContain('get_workspace_review_context');
        expect(tools).toContain('list_workspace_review_files');
        expect(tools).toContain('get_workspace_review_diff_page');
        expect(tools).toContain('write_workspace_review_artifact');
        expect(tools).toContain('write_workspace_review_hunk_annotations');
        expect(tools).toContain('complete_workspace_review_run');
        expect(tools).not.toContain('get_agent_task');
        expect(tools).not.toContain('list_agent_tasks');
        expect(tools).not.toContain('search_memories');
    });
    it('automation setup allowlist mirrors canonical session-bound automation tools', () => {
        const tools = toolsByAgent()[AUTOMATION_SETUP];
        expect(tools).toEqual(loadCanonicalMcpTools(AUTOMATION_SETUP));
        expect(tools).toEqual([
            'fs_read_file',
            'fs_list_dir',
            'fs_grep',
            'fs_glob',
            'list_projects',
            'ask_user_question',
            'get_artifact',
            'get_automation',
            'update_automation',
            'verify_automation_decomposition',
            'finalize_automation',
            'run_automation_now',
            'pause_automation',
            'resume_automation',
            'cancel_automation_run',
            'cancel_automation',
            'restart_automation',
            'retry_automation_judge',
            'retry_automation_plan_judge',
            'skip_automation_judge',
            'get_automation_publish_status',
            'check_automation_publish_readiness',
            'update_automation_from_base',
            'publish_automation_workspace',
        ]);
        setAgentType(AUTOMATION_SETUP);
        const filteredToolNames = getFilteredTools().map((tool) => tool.name);
        expect(filteredToolNames).toHaveLength(tools.length);
        expect(filteredToolNames).toEqual(expect.arrayContaining(tools));
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
    it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
        expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('delete_task_proposal');
    });
    it('should be returned by getFilteredTools for ralphx-ideation', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const toolNames = getFilteredTools().map((t) => t.name);
        expect(toolNames).toContain('delete_task_proposal');
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
        it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
            expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('get_acceptance_status');
        });
        it('should be returned by getFilteredTools for ralphx-ideation', () => {
            setAgentType(ORCHESTRATOR_IDEATION);
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
        it('should NOT be in TOOL_ALLOWLIST for ralphx-ideation-readonly', () => {
            expect(toolsByAgent()[ORCHESTRATOR_IDEATION_READONLY]).not.toContain('get_pending_confirmations');
        });
        it('should be returned by getFilteredTools for ralphx-ideation', () => {
            setAgentType(ORCHESTRATOR_IDEATION);
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
    it('repair agent allowlist includes review artifact fetch and completion tools', () => {
        const tools = toolsByAgent()[AGENT_WORKSPACE_REPAIR];
        expect(tools).toEqual(loadCanonicalMcpTools(AGENT_WORKSPACE_REPAIR));
        expect(tools).toContain('get_artifact');
        expect(tools).toContain('complete_agent_workspace_repair');
        expect(tools).not.toContain('write_workspace_review_artifact');
        expect(tools).not.toContain('complete_workspace_review_run');
    });
});
describe('agent workspace publish tools', () => {
    const allTools = getAllTools();
    const publishTools = [
        'get_agent_workspace_publish_status',
        'check_agent_workspace_publish_readiness',
        'update_agent_workspace_from_base',
        'publish_agent_workspace',
    ];
    it.each(publishTools)('%s should exist in ALL_TOOLS', (toolName) => {
        expect(allTools.find((t) => t.name === toolName)).toBeDefined();
    });
    it.each(publishTools)('%s should accept the current workspace conversation context', (toolName) => {
        const tool = allTools.find((t) => t.name === toolName);
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('conversation_id');
        expect(tool?.inputSchema.required ?? []).not.toContain('conversation_id');
    });
    it('exposes publish tools to the general worker only through canonical metadata', () => {
        setAgentType(GENERAL_WORKER);
        process.env.RALPHX_COORDINATION_MODE = 'rx_native_workflow';
        try {
            const toolNames = getFilteredTools().map((tool) => tool.name);
            expect(new Set(toolNames)).toEqual(new Set((loadCanonicalMcpTools(GENERAL_WORKER) ?? []).filter((tool) => tool !== 'team_send_message' && tool !== 'team_roster')));
            for (const toolName of publishTools) {
                expect(toolNames).toContain(toolName);
            }
        }
        finally {
            delete process.env.RALPHX_COORDINATION_MODE;
        }
    });
    it('keeps publish tools off unrelated agent surfaces', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        for (const toolName of publishTools) {
            expect(toolNames).not.toContain(toolName);
        }
    });
});
describe('agent workspace PR fix tools', () => {
    const allTools = getAllTools();
    const prFixTools = [
        'get_agent_workspace_pr_fix_context',
        'read_agent_workspace_pr_comment',
        'complete_agent_workspace_pr_fix',
    ];
    it.each(prFixTools)('%s should exist in ALL_TOOLS', (toolName) => {
        expect(allTools.find((t) => t.name === toolName)).toBeDefined();
    });
    it('requires a conversation id and summary when completing a PR fix', () => {
        const tool = allTools.find((t) => t.name === 'complete_agent_workspace_pr_fix');
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('conversation_id');
        expect(tool?.inputSchema.properties).toHaveProperty('summary');
        expect(tool?.inputSchema.properties).toHaveProperty('blocker');
        expect(tool?.inputSchema.properties).toHaveProperty('fix_commit_sha');
        expect(tool?.inputSchema.properties).not.toHaveProperty('created_by_run_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('agent_run_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('run_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('orchestration_id');
        expect(tool?.inputSchema.properties).toMatchObject({
            fix_commit_sha: {
                description: expect.stringContaining('Required when blocker is absent'),
                pattern: '^[0-9a-f]{40}$',
            },
        });
        expect(tool?.inputSchema.required).toEqual(expect.arrayContaining(['conversation_id', 'summary']));
    });
    it('exposes PR fix tools only through the PR fixer canonical metadata', () => {
        setAgentType(AGENT_WORKSPACE_PR_FIXER);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(new Set(toolNames)).toEqual(new Set(loadCanonicalMcpTools(AGENT_WORKSPACE_PR_FIXER)));
        for (const toolName of prFixTools) {
            expect(toolNames).toContain(toolName);
        }
    });
});
describe('agent workspace PR description tool', () => {
    const allTools = getAllTools();
    const tool = allTools.find((t) => t.name === 'submit_agent_workspace_pr_description');
    it('should exist in ALL_TOOLS', () => {
        expect(tool).toBeDefined();
    });
    it('should require conversation and decision fields', () => {
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('conversation_id');
        expect(tool?.inputSchema.properties).toHaveProperty('title');
        expect(tool?.inputSchema.properties).toHaveProperty('body_markdown');
        expect(tool?.inputSchema.properties).toHaveProperty('decision');
        expect(tool?.inputSchema.required).toEqual(expect.arrayContaining(['conversation_id', 'decision']));
    });
    it('limits existing PR body patches to the supplied editable region', () => {
        const bodyDescription = inputSchemaProperties('submit_agent_workspace_pr_description').body_markdown?.description;
        expect(bodyDescription).toContain('patch_allowed=true');
        expect(bodyDescription).toContain('RalphX-managed Plan/signature');
        expect(bodyDescription).toContain('trailing integration block');
    });
    it('routes PR description submissions to the agent workspace endpoint', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callSubmitAgentWorkspacePrDescriptionTool(callTauri, {
            conversation_id: 'conversation-1',
            decision: 'patch',
            title: 'Generated title',
            body_markdown: '## Summary\n\nGenerated body',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/pr-description', {
            title: 'Generated title',
            body_markdown: '## Summary\n\nGenerated body',
            decision: 'patch',
        });
    });
});
describe('plan complexity assessment tool', () => {
    const allTools = getAllTools();
    const tool = allTools.find((t) => t.name === 'submit_plan_complexity_assessment');
    it('should exist in ALL_TOOLS', () => {
        expect(tool).toBeDefined();
    });
    it('should require the current approved plan assessment fields', () => {
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('session_id');
        expect(tool?.inputSchema.properties).toHaveProperty('artifact_id');
        expect(tool?.inputSchema.properties).toHaveProperty('artifact_version');
        expect(tool?.inputSchema.properties).toHaveProperty('level');
        expect(tool?.inputSchema.properties).toHaveProperty('score');
        expect(tool?.inputSchema.properties).toHaveProperty('recommended_action');
        expect(tool?.inputSchema.properties).toHaveProperty('confidence');
        expect(tool?.inputSchema.properties).toHaveProperty('reason_summary');
        expect(tool?.inputSchema.required).toEqual(expect.arrayContaining([
            'session_id',
            'artifact_id',
            'artifact_version',
            'level',
            'score',
            'recommended_action',
            'confidence',
            'reason_summary',
        ]));
    });
    it('exposes the submit tool only through the utility assessor metadata', () => {
        setAgentType(PLAN_COMPLEXITY_ASSESSOR);
        const toolNames = getFilteredTools().map((candidate) => candidate.name);
        expect(toolNames).toEqual(loadCanonicalMcpTools(PLAN_COMPLEXITY_ASSESSOR));
        expect(toolNames).toEqual(['submit_plan_complexity_assessment']);
    });
});
describe('agent workspace publish tool transport', () => {
    it('routes publish status reads to the agent workspace endpoint', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callGetAgentWorkspacePublishStatusTool(callTauriGet, {
            conversation_id: 'conversation-1',
        })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-1/publish-status');
    });
    it('defaults publish status reads to the current runtime workspace conversation', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callGetAgentWorkspacePublishStatusTool(callTauriGet, {}, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/publish-status');
    });
    it('routes publish readiness checks to the agent workspace endpoint', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callCheckAgentWorkspacePublishReadinessTool(callTauriGet, {
            conversation_id: 'conversation-1',
        })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-1/publish-readiness');
    });
    it('defaults publish readiness checks to the current runtime workspace conversation', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callCheckAgentWorkspacePublishReadinessTool(callTauriGet, {}, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/publish-readiness');
    });
    it('routes base updates to the agent workspace endpoint', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callUpdateAgentWorkspaceFromBaseTool(callTauri, {
            conversation_id: 'conversation-1',
            base_ref_kind: 'local_branch',
            base_ref: 'feature/base',
            base_display_name: 'feature/base',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/update-from-base', {
            base_ref_kind: 'local_branch',
            base_ref: 'feature/base',
            base_display_name: 'feature/base',
            created_by_run_id: undefined,
        });
    });
    it('defaults base updates to the current runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callUpdateAgentWorkspaceFromBaseTool(callTauri, { base_ref_kind: 'project_default' }, {
            parentConversationId: 'conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/update-from-base', {
            base_ref_kind: 'project_default',
            base_ref: undefined,
            base_display_name: undefined,
            created_by_run_id: 'run-from-runtime',
        });
    });
    it('routes publish requests to the agent workspace endpoint', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callPublishAgentWorkspaceTool(callTauri, {
            conversation_id: 'conversation-1',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/publish', {});
    });
    it('defaults publish requests to the current runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callPublishAgentWorkspaceTool(callTauri, {}, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/publish', {});
    });
    it('reports a clear error when no conversation id is available', async () => {
        await expect(callPublishAgentWorkspaceTool(vi.fn(), {})).rejects.toThrow('publish_agent_workspace requires conversation_id because RalphX did not provide the current workspace conversation id');
    });
    it('routes PR fix context reads to the agent workspace endpoint', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callGetAgentWorkspacePrFixContextTool(callTauriGet, {
            conversation_id: 'conversation-1',
        })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-1/pr-fix-context');
    });
    it('routes Review PR context reads to the current runtime workspace conversation', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callGetPrReviewContextTool(callTauriGet, {}, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/pr-review-context');
    });
    it('routes workspace Review context reads to the current runtime workspace conversation', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callGetWorkspaceReviewContextTool(callTauriGet, {}, {
            parentConversationId: 'conversation-from-runtime',
            conversationId: 'review-conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/workspace-review-context?include_review_packet=true', {
            headers: {
                'x-ralphx-agent-run-id': 'run-from-runtime',
                'x-ralphx-conversation-id': 'review-conversation-from-runtime',
            },
        });
    });
    it('routes encoded workspace Review file and diff pages with runtime identity', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        const runtimeContext = {
            parentConversationId: 'conversation-from-runtime',
            conversationId: 'review-conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        };
        await callListWorkspaceReviewFilesTool(callTauriGet, { cursor: 'cursor/with+symbols=', limit: 75 }, runtimeContext);
        await callGetWorkspaceReviewDiffPageTool(callTauriGet, { path: 'src/file with spaces.rs', source: 'unstaged', limit: 120 }, runtimeContext);
        const options = {
            headers: {
                'x-ralphx-agent-run-id': 'run-from-runtime',
                'x-ralphx-conversation-id': 'review-conversation-from-runtime',
            },
        };
        expect(callTauriGet).toHaveBeenNthCalledWith(1, 'agent-workspaces/conversation-from-runtime/workspace-review-files?cursor=cursor%2Fwith%2Bsymbols%3D&limit=75', options);
        expect(callTauriGet).toHaveBeenNthCalledWith(2, 'agent-workspaces/conversation-from-runtime/workspace-review-diff-page?path=src%2Ffile+with+spaces.rs&source=unstaged&limit=120', options);
    });
    it('ignores undeclared caller workspace identity for Review paging tools', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        const runtimeContext = {
            parentConversationId: 'conversation-from-runtime',
            conversationId: 'review-conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        };
        await callListWorkspaceReviewFilesTool(callTauriGet, { conversation_id: 'caller-controlled-conversation' }, runtimeContext);
        await callGetWorkspaceReviewDiffPageTool(callTauriGet, {
            conversation_id: 'caller-controlled-conversation',
            path: 'src/lib.rs',
            source: 'unstaged',
        }, runtimeContext);
        expect(callTauriGet).toHaveBeenNthCalledWith(1, 'agent-workspaces/conversation-from-runtime/workspace-review-files', expect.anything());
        expect(callTauriGet).toHaveBeenNthCalledWith(2, 'agent-workspaces/conversation-from-runtime/workspace-review-diff-page?path=src%2Flib.rs&source=unstaged', expect.anything());
    });
    it('keeps workspace Review paging identity and target fields off model schemas', () => {
        for (const toolName of [
            'list_workspace_review_files',
            'get_workspace_review_diff_page',
        ]) {
            const tool = AGENT_WORKSPACE_TOOLS.find((candidate) => candidate.name === toolName);
            expect(tool, toolName).toBeDefined();
            const properties = (tool?.inputSchema.properties ?? {});
            for (const forbidden of [
                'conversation_id',
                'run_id',
                'target_scope',
                'head_sha',
                'diff_fingerprint',
                'base_ref',
                'head_ref',
            ]) {
                expect(properties).not.toHaveProperty(forbidden);
            }
        }
        const diffTool = AGENT_WORKSPACE_TOOLS.find((candidate) => candidate.name === 'get_workspace_review_diff_page');
        expect(diffTool?.inputSchema).not.toHaveProperty('oneOf');
    });
    it('rejects mixed or incomplete workspace Review diff page selections', async () => {
        const callTauriGet = vi.fn();
        const runtimeContext = { parentConversationId: 'conversation-from-runtime' };
        await expect(callGetWorkspaceReviewDiffPageTool(callTauriGet, { cursor: 'cursor', path: 'src/lib.rs', source: 'unstaged' }, runtimeContext)).rejects.toThrow('either path and source');
        await expect(callGetWorkspaceReviewDiffPageTool(callTauriGet, { path: 'src/lib.rs' }, runtimeContext)).rejects.toThrow('either path and source');
        await expect(callGetWorkspaceReviewDiffPageTool(callTauriGet, { cursor: 'cursor', path: 'src/lib.rs' }, runtimeContext)).rejects.toThrow('either path and source');
        expect(callTauriGet).not.toHaveBeenCalled();
    });
    it('routes workspace Review artifact writes to the runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callWriteWorkspaceReviewArtifactTool(callTauri, {
            content: '## Summary\n\nLooks good.',
            requested_changes_content: '## Result\n\nNo changes requested.',
            target_scope: 'workspace_delta',
            head_sha: 'abc123',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'event-id-from-context',
        }, {
            parentConversationId: 'conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/workspace-review-artifact', {
            title: undefined,
            content: '## Summary\n\nLooks good.',
            requested_changes_title: undefined,
            requested_changes_content: '## Result\n\nNo changes requested.',
            target_scope: 'workspace_delta',
            head_sha: 'abc123',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'run-from-runtime',
        });
    });
    it('routes workspace Review hunk annotation writes to the runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        const annotations = [
            {
                path: 'src/lib.rs',
                source: 'committed',
                hunk_header: '@@ -1,1 +1,2 @@',
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                title: 'Updates lib',
                message: 'Explains the changed hunk.',
                level: 'notice',
            },
        ];
        await expect(callWriteWorkspaceReviewHunkAnnotationsTool(callTauri, {
            target_scope: 'workspace_delta',
            head_sha: 'abc123',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'event-id-from-context',
            annotations,
        }, {
            parentConversationId: 'conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/workspace-review-hunk-annotations', {
            target_scope: 'workspace_delta',
            head_sha: 'abc123',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'run-from-runtime',
            annotations,
        });
    });
    it('routes workspace Review run completion to the runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callCompleteWorkspaceReviewRunTool(callTauri, {
            outcome: 'passed',
            summary: 'Review completed',
            blocker: undefined,
            created_by_run_id: 'event-id-from-context',
        }, {
            parentConversationId: 'conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/complete-workspace-review-run', {
            outcome: 'passed',
            summary: 'Review completed',
            blocker: undefined,
            created_by_run_id: 'run-from-runtime',
        });
    });
    it('does not expose workspace Review run bookkeeping in model-facing schemas', () => {
        const workspaceReviewToolNames = [
            'write_workspace_review_artifact',
            'write_workspace_review_hunk_annotations',
            'complete_workspace_review_run',
        ];
        for (const toolName of workspaceReviewToolNames) {
            const tool = AGENT_WORKSPACE_TOOLS.find((candidate) => candidate.name === toolName);
            expect(tool, toolName).toBeDefined();
            const schema = tool?.inputSchema;
            expect(schema.properties ?? {}).not.toHaveProperty('created_by_run_id');
            expect(schema.required ?? []).not.toContain('created_by_run_id');
        }
    });
    it('requires Overview and Requested Changes in one workspace Review write', () => {
        const tool = AGENT_WORKSPACE_TOOLS.find((candidate) => candidate.name === 'write_workspace_review_artifact');
        const schema = tool?.inputSchema;
        expect(schema.required).toEqual(expect.arrayContaining(['content', 'requested_changes_content']));
    });
    it('routes proposed Review PR actions to the agent workspace endpoint', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callProposePrReviewActionTool(callTauri, {
            conversation_id: 'conversation-1',
            head_sha: 'abc123',
            proposed_action: 'request_changes',
            summary: 'Found blocking issues',
            review_body: 'Please fix the blocking issues.',
            findings_json: '[{"path":"src/lib.rs"}]',
            created_by_run_id: 'run-1',
        }, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/pr-review-actions', {
            head_sha: 'abc123',
            proposed_action: 'request_changes',
            summary: 'Found blocking issues',
            review_body: 'Please fix the blocking issues.',
            findings_json: '[{"path":"src/lib.rs"}]',
            created_by_run_id: 'run-1',
        });
    });
    it('routes Review PR run completion to the runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callCompletePrReviewRunTool(callTauri, {
            head_sha: 'abc123',
            outcome: 'request_changes',
            summary: 'Review completed',
            blocker: undefined,
            created_by_run_id: 'run-1',
        }, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/complete-pr-review-run', {
            head_sha: 'abc123',
            outcome: 'request_changes',
            summary: 'Review completed',
            blocker: undefined,
            created_by_run_id: 'run-1',
        });
    });
    it('routes Review PR artifact writes to the runtime workspace conversation', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callWritePrReviewArtifactTool(callTauri, {
            title: 'PR #42 Review',
            content: '## Review\n\nLooks good.',
            head_sha: 'abc123',
            created_by_run_id: 'run-1',
        }, { parentConversationId: 'conversation-from-runtime' })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/pr-review-artifact', {
            title: 'PR #42 Review',
            content: '## Review\n\nLooks good.',
            head_sha: 'abc123',
            created_by_run_id: 'run-1',
        });
    });
    it('routes PR comment reads to the encoded comment endpoint', async () => {
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        await expect(callReadAgentWorkspacePrCommentTool(callTauriGet, {
            conversation_id: 'conversation-1',
            comment_id: 'comment/with/slash',
        })).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-1/pr-comments/comment%2Fwith%2Fslash');
    });
    it('dispatches Review PR tools through the generic agent workspace router', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        const callTauriGet = vi.fn().mockResolvedValue({ success: true });
        const runtimeContext = {
            parentConversationId: 'conversation-from-runtime',
            agentRunId: 'run-from-runtime',
        };
        await expect(callAgentWorkspaceTool('get_pr_review_context', callTauri, callTauriGet, {}, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('propose_pr_review_action', callTauri, callTauriGet, { summary: 'Ready to submit' }, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('complete_pr_review_run', callTauri, callTauriGet, { outcome: 'approved' }, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('write_pr_review_artifact', callTauri, callTauriGet, { content: '## Review' }, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('get_workspace_review_context', callTauri, callTauriGet, {}, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('write_workspace_review_artifact', callTauri, callTauriGet, {
            content: '## Summary',
            requested_changes_content: '## Result\n\nNo changes requested.',
            target_scope: 'selected_source',
            head_sha: 'head-sha',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'run-1',
        }, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('write_workspace_review_hunk_annotations', callTauri, callTauriGet, {
            target_scope: 'selected_source',
            head_sha: 'head-sha',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'run-1',
            annotations: [],
        }, runtimeContext)).resolves.toEqual({ success: true });
        await expect(callAgentWorkspaceTool('complete_workspace_review_run', callTauri, callTauriGet, { summary: 'Done', outcome: 'passed', created_by_run_id: 'run-1' }, runtimeContext)).resolves.toEqual({ success: true });
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/pr-review-context');
        expect(callTauriGet).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/workspace-review-context?include_review_packet=true');
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/pr-review-actions', {
            head_sha: undefined,
            proposed_action: undefined,
            summary: 'Ready to submit',
            review_body: undefined,
            findings_json: undefined,
            created_by_run_id: undefined,
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/complete-pr-review-run', {
            head_sha: undefined,
            outcome: 'approved',
            summary: undefined,
            blocker: undefined,
            created_by_run_id: undefined,
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/pr-review-artifact', {
            title: undefined,
            content: '## Review',
            head_sha: undefined,
            created_by_run_id: undefined,
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/workspace-review-artifact', {
            title: undefined,
            content: '## Summary',
            requested_changes_title: undefined,
            requested_changes_content: '## Result\n\nNo changes requested.',
            target_scope: 'selected_source',
            head_sha: 'head-sha',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'run-from-runtime',
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/workspace-review-hunk-annotations', {
            target_scope: 'selected_source',
            head_sha: 'head-sha',
            diff_fingerprint: 'fingerprint-1',
            created_by_run_id: 'run-from-runtime',
            annotations: [],
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-from-runtime/complete-workspace-review-run', {
            outcome: 'passed',
            summary: 'Done',
            blocker: undefined,
            created_by_run_id: 'run-from-runtime',
        });
    });
    it('routes a PR fix blocker without fabricating a commit SHA', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callCompleteAgentWorkspacePrFixTool(callTauri, {
            conversation_id: 'conversation-1',
            summary: 'Fixed failing tests',
            blocker: 'Needs maintainer decision',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/complete-pr-fix', {
            summary: 'Fixed failing tests',
            blocker: 'Needs maintainer decision',
            fix_commit_sha: undefined,
        });
    });
    it('forwards the exact committed HEAD for successful PR fix completion', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        const fixCommitSha = 'c'.repeat(40);
        await callCompleteAgentWorkspacePrFixTool(callTauri, {
            conversation_id: 'conversation-1',
            summary: 'Fixed failing tests',
            fix_commit_sha: fixCommitSha,
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/complete-pr-fix', {
            summary: 'Fixed failing tests',
            blocker: undefined,
            fix_commit_sha: fixCommitSha,
        });
    });
    it('injects the runtime run identity for PR fix completion and preserves supersession responses', async () => {
        const superseded = {
            success: false,
            code: 'superseded',
            message: 'A newer PR fix completion superseded this request.',
        };
        const callTauri = vi.fn().mockResolvedValue(superseded);
        await expect(callCompleteAgentWorkspacePrFixTool(callTauri, {
            conversation_id: 'conversation-1',
            summary: 'Fixed failing tests',
            fix_commit_sha: 'c'.repeat(40),
            created_by_run_id: 'caller-controlled-run-id',
        }, { agentRunId: 'run-from-runtime' })).resolves.toBe(superseded);
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/complete-pr-fix', {
            summary: 'Fixed failing tests',
            blocker: undefined,
            fix_commit_sha: 'c'.repeat(40),
            created_by_run_id: 'run-from-runtime',
        });
    });
    it('omits the hidden PR fix run identity when runtime context has no agent run', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await callCompleteAgentWorkspacePrFixTool(callTauri, {
            conversation_id: 'conversation-1',
            summary: 'Blocked on maintainer decision',
            blocker: 'Needs maintainer decision',
        });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/complete-pr-fix', {
            summary: 'Blocked on maintainer decision',
            blocker: 'Needs maintainer decision',
            fix_commit_sha: undefined,
            created_by_run_id: undefined,
        });
    });
    it.each([
        [
            'get_agent_workspace_publish_status',
            'get',
            'agent-workspaces/conversation-1/publish-status',
            undefined,
        ],
        [
            'check_agent_workspace_publish_readiness',
            'get',
            'agent-workspaces/conversation-1/publish-readiness',
            undefined,
        ],
        [
            'update_agent_workspace_from_base',
            'post',
            'agent-workspaces/conversation-1/update-from-base',
            {
                base_ref_kind: 'local_branch',
                base_ref: 'feature/base',
                base_display_name: 'feature/base',
                created_by_run_id: 'run-from-runtime',
            },
        ],
        ['publish_agent_workspace', 'post', 'agent-workspaces/conversation-1/publish', {}],
        [
            'get_agent_workspace_pr_fix_context',
            'get',
            'agent-workspaces/conversation-1/pr-fix-context',
            undefined,
        ],
        [
            'read_agent_workspace_pr_comment',
            'get',
            'agent-workspaces/conversation-1/pr-comments/comment-1',
            undefined,
        ],
        [
            'write_pr_review_artifact',
            'post',
            'agent-workspaces/conversation-1/pr-review-artifact',
            {
                title: 'Generated title',
                content: '## Summary\n\nGenerated body',
                head_sha: 'head-sha',
                created_by_run_id: 'run-1',
            },
        ],
        [
            'get_workspace_review_context',
            'get',
            'agent-workspaces/conversation-1/workspace-review-context?include_review_packet=true',
            undefined,
        ],
        [
            'write_workspace_review_artifact',
            'post',
            'agent-workspaces/conversation-1/workspace-review-artifact',
            {
                title: 'Generated title',
                content: '## Summary\n\nGenerated body',
                requested_changes_title: undefined,
                requested_changes_content: '## Requested Changes\n\nGenerated blueprint',
                target_scope: 'workspace_delta',
                head_sha: 'head-sha',
                diff_fingerprint: 'fingerprint-1',
                created_by_run_id: 'run-from-runtime',
            },
        ],
        [
            'write_workspace_review_hunk_annotations',
            'post',
            'agent-workspaces/conversation-1/workspace-review-hunk-annotations',
            {
                target_scope: 'workspace_delta',
                head_sha: 'head-sha',
                diff_fingerprint: 'fingerprint-1',
                created_by_run_id: 'run-from-runtime',
                annotations: undefined,
            },
        ],
        [
            'complete_workspace_review_run',
            'post',
            'agent-workspaces/conversation-1/complete-workspace-review-run',
            {
                outcome: 'passed',
                summary: 'Resolved conflicts',
                blocker: 'Needs maintainer decision',
                created_by_run_id: 'run-from-runtime',
            },
        ],
        [
            'complete_agent_workspace_pr_fix',
            'post',
            'agent-workspaces/conversation-1/complete-pr-fix',
            {
                summary: 'Resolved conflicts',
                blocker: 'Needs maintainer decision',
                fix_commit_sha: 'a'.repeat(40),
                created_by_run_id: 'run-from-runtime',
            },
        ],
        [
            'complete_agent_workspace_repair',
            'post',
            'agent-workspaces/conversation-1/complete-repair',
            {
                repair_commit_sha: 'a'.repeat(40),
                resolved_base_ref: 'main',
                resolved_base_commit: 'b'.repeat(40),
                summary: 'Resolved conflicts',
            },
        ],
        [
            'submit_agent_workspace_pr_description',
            'post',
            'agent-workspaces/conversation-1/pr-description',
            {
                title: 'Generated title',
                body_markdown: '## Summary\n\nGenerated body',
            },
        ],
    ])('routes %s through the centralized agent workspace dispatcher', async (toolName, method, expectedPath, expectedBody) => {
        const callTauri = vi.fn().mockResolvedValue({ ok: 'post' });
        const callTauriGet = vi.fn().mockResolvedValue({ ok: 'get' });
        const args = {
            conversation_id: 'conversation-1',
            base_ref_kind: 'local_branch',
            base_ref: 'feature/base',
            base_display_name: 'feature/base',
            comment_id: 'comment-1',
            repair_commit_sha: 'a'.repeat(40),
            resolved_base_ref: 'main',
            resolved_base_commit: 'b'.repeat(40),
            summary: 'Resolved conflicts',
            blocker: 'Needs maintainer decision',
            fix_commit_sha: 'a'.repeat(40),
            title: 'Generated title',
            body_markdown: '## Summary\n\nGenerated body',
            content: '## Summary\n\nGenerated body',
            requested_changes_content: '## Requested Changes\n\nGenerated blueprint',
            target_scope: 'workspace_delta',
            head_sha: 'head-sha',
            diff_fingerprint: 'fingerprint-1',
            outcome: 'passed',
            created_by_run_id: 'run-1',
        };
        await expect(callAgentWorkspaceTool(toolName, callTauri, callTauriGet, args, {
            agentRunId: 'run-from-runtime',
        })).resolves.toEqual({ ok: method });
        expect(isAgentWorkspaceToolName(toolName)).toBe(true);
        if (method === 'get') {
            expect(callTauriGet).toHaveBeenCalledWith(expectedPath);
            expect(callTauri).not.toHaveBeenCalled();
        }
        else {
            expect(callTauri).toHaveBeenCalledWith(expectedPath, expectedBody);
            expect(callTauriGet).not.toHaveBeenCalled();
        }
    });
    it('rejects unknown agent workspace dispatch names', async () => {
        await expect(callAgentWorkspaceTool('unknown_agent_workspace_tool', vi.fn(), vi.fn(), {})).rejects.toThrow('Unsupported agent workspace tool');
        expect(isAgentWorkspaceToolName('unknown_agent_workspace_tool')).toBe(false);
    });
});
describe('agent workspace repair tool transport', () => {
    it('routes repair completion to the agent workspace endpoint', async () => {
        const callTauri = vi.fn().mockResolvedValue({ success: true });
        await expect(callCompleteAgentWorkspaceRepairTool(callTauri, {
            conversation_id: 'conversation-1',
            repair_commit_sha: 'a'.repeat(40),
            resolved_base_ref: 'main',
            resolved_base_commit: 'b'.repeat(40),
            summary: 'Resolved conflicts',
        })).resolves.toEqual({ success: true });
        expect(callTauri).toHaveBeenCalledWith('agent-workspaces/conversation-1/complete-repair', {
            repair_commit_sha: 'a'.repeat(40),
            resolved_base_ref: 'main',
            resolved_base_commit: 'b'.repeat(40),
            summary: 'Resolved conflicts',
        });
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
    it('delegate_start should hide session selection and require only agent_name and prompt', () => {
        const tool = allTools.find((entry) => entry.name === 'delegate_start');
        expect(tool?.inputSchema.type).toBe('object');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_session_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_turn_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_message_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_conversation_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('parent_agent_run_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('caller_agent_run_id');
        expect(tool?.inputSchema.properties).toHaveProperty('parent_tool_use_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('delegated_session_id');
        expect(tool?.inputSchema.properties).not.toHaveProperty('child_session_id');
        expect(tool?.inputSchema.properties).toHaveProperty('task_ref');
        expect(tool?.inputSchema.additionalProperties).toBe(false);
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
    it.each([
        ORCHESTRATOR_IDEATION,
        ORCHESTRATOR_IDEATION_READONLY,
        GENERAL_EXPLORER,
        GENERAL_WORKER,
        PR_REVIEWER,
    ])('%s should expose delegation bridge tools', (agent) => {
        expect(toolsByAgent()[agent]).toContain('delegate_start');
        expect(toolsByAgent()[agent]).toContain('delegate_wait');
        expect(toolsByAgent()[agent]).toContain('delegate_cancel');
    });
    it('PR_REVIEWER should expose get_artifact for selected plan references', () => {
        expect(toolsByAgent()[PR_REVIEWER]).toEqual(loadCanonicalMcpTools(PR_REVIEWER));
        expect(toolsByAgent()[PR_REVIEWER]).toContain('get_artifact');
    });
    it.each([WORKER, REVIEWER, MERGER])('%s should expose delegation bridge tools in the fallback allowlist', (agent) => {
        expect(toolsByAgent()[agent]).toContain('delegate_start');
        expect(toolsByAgent()[agent]).toContain('delegate_wait');
        expect(toolsByAgent()[agent]).toContain('delegate_cancel');
    });
    it.each([
        ORCHESTRATOR_IDEATION,
        ORCHESTRATOR_IDEATION_READONLY,
        GENERAL_EXPLORER,
        GENERAL_WORKER,
        PR_REVIEWER,
        WORKER,
        REVIEWER,
        MERGER,
    ])('%s should return delegate_start from getFilteredTools', (agent) => {
        setAgentType(agent);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('delegate_start');
    });
    it('ideation orchestrator receives the native delegation bridge tools', () => {
        setAgentType(ORCHESTRATOR_IDEATION);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('delegate_start');
        expect(toolNames).toContain('delegate_wait');
        expect(toolNames).toContain('delegate_cancel');
    });
});
// ===========================================================================
// RalphX native agent task tools
// ===========================================================================
describe('agent task tools', () => {
    const allTools = getAllTools();
    it.each([
        'create_agent_task',
        'get_agent_task',
        'list_agent_tasks',
        'update_agent_task',
        'claim_agent_task',
        'complete_agent_task',
        'get_delegate_assignment',
        'complete_delegate_assignment',
        'release_delegate_assignment',
    ])('%s should exist in ALL_TOOLS', (toolName) => {
        expect(allTools.find((tool) => tool.name === toolName)).toBeDefined();
    });
    it('create_agent_task should require title and details without caller context fields', () => {
        const tool = allTools.find((entry) => entry.name === 'create_agent_task');
        expect(tool?.inputSchema.required).toEqual(expect.arrayContaining(['title', 'details']));
        expect(tool?.inputSchema.properties).not.toHaveProperty('context_type');
        expect(tool?.inputSchema.properties).not.toHaveProperty('actor_agent');
    });
    it('agent task tool descriptions explain single-task cleanup and decomposition', () => {
        const createTool = allTools.find((entry) => entry.name === 'create_agent_task');
        const updateTool = allTools.find((entry) => entry.name === 'update_agent_task');
        const claimTool = allTools.find((entry) => entry.name === 'claim_agent_task');
        const completeTool = allTools.find((entry) => entry.name === 'complete_agent_task');
        expect(createTool?.description).toContain('Do not create a task for genuinely single-step work');
        expect(createTool?.description).toContain('decomposed into multiple concrete tasks');
        expect(updateTool?.description).toContain('state=dropped');
        expect(claimTool?.description).toContain('only one meaningful task');
        expect(completeTool?.description).toContain('single-task ledger');
    });
    it.each([ORCHESTRATOR_IDEATION, CHAT_PROJECT, GENERAL_WORKER, WORKER])('%s should expose writable agent task tools', (agent) => {
        expect(toolsByAgent()[agent]).toContain('create_agent_task');
        expect(toolsByAgent()[agent]).toContain('list_agent_tasks');
        expect(toolsByAgent()[agent]).toContain('complete_agent_task');
    });
    it('getFilteredTools should include agent task tools for general worker', () => {
        setAgentType(GENERAL_WORKER);
        const toolNames = getFilteredTools().map((tool) => tool.name);
        expect(toolNames).toContain('create_agent_task');
        expect(toolNames).toContain('list_agent_tasks');
    });
    it('delegate assignment schemas expose no orchestration identity fields', () => {
        for (const toolName of [
            'get_delegate_assignment',
            'complete_delegate_assignment',
            'release_delegate_assignment',
        ]) {
            const tool = allTools.find((entry) => entry.name === toolName);
            expect(tool).toBeDefined();
            for (const forbidden of [
                'delegated_session_id',
                'conversation_id',
                'agent_run_id',
                'assignment_id',
                'task_list_id',
            ]) {
                expect(tool?.inputSchema.properties).not.toHaveProperty(forbidden);
            }
        }
    });
    it.each([GENERAL_EXPLORER, GENERAL_WORKER])('%s should expose delegate-local and narrow assignment lifecycles', (agent) => {
        expect(toolsByAgent()[agent]).toEqual(loadCanonicalMcpTools(agent));
        expect(toolsByAgent()[agent]).toContain('create_agent_task');
        expect(toolsByAgent()[agent]).toContain('complete_agent_task');
        expect(toolsByAgent()[agent]).toContain('get_delegate_assignment');
        expect(toolsByAgent()[agent]).toContain('complete_delegate_assignment');
        expect(toolsByAgent()[agent]).toContain('release_delegate_assignment');
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
    it.each(parentContextSpecialists)('%s should include get_parent_session_context', (agent) => {
        expect(toolsByAgent()[agent]).toContain('get_parent_session_context');
    });
    it('IDEATION_SPECIALIST_BACKEND should include get_parent_session_context', () => {
        expect(toolsByAgent()[IDEATION_SPECIALIST_BACKEND]).toContain('get_parent_session_context');
    });
    it.each([
        IDEATION_SPECIALIST_BACKEND,
        WORKER,
        WORKER,
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
});
describe('persona builder tool registry', () => {
    it('includes exactly the two persona draft tools in ALL_TOOLS', () => {
        const allToolNames = getAllTools().map((tool) => tool.name);
        const personaToolNames = allToolNames.filter((toolName) => toolName === 'save_persona_draft' || toolName === 'get_persona_draft');
        expect(personaToolNames).toEqual(['save_persona_draft', 'get_persona_draft']);
        expect(new Set(allToolNames).size).toBe(allToolNames.length);
    });
});
//# sourceMappingURL=tools.test.js.map