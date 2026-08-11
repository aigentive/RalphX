/**
 * Handler for ask_user_question MCP tool
 *
 * Mirrors the permission_request pattern:
 * 1. POST /api/question/request — registers question, emits Tauri event
 * 2. GET /api/question/await/:request_id — long-polls for user answer (about 5 min timeout)
 * 3. Returns answer to agent as tool result
 */
interface QuestionOption {
    label: string;
    value?: string;
    description?: string;
}
interface QuestionPrompt {
    id?: string;
    question?: string;
    header?: string;
    options?: QuestionOption[];
    multi_select?: boolean;
    allow_skip?: boolean;
}
export interface AskUserQuestionArgs {
    session_id?: string;
    question?: string;
    header?: string;
    options?: QuestionOption[];
    multi_select?: boolean;
    allow_skip?: boolean;
    questions?: QuestionPrompt[];
    metadata?: Record<string, unknown>;
}
export interface ProposePlanModeArgs {
    conversation_id?: string;
    current_mode?: string;
    reason?: string;
    question?: string;
}
type ToolTextResult = {
    content: Array<{
        type: "text";
        text: string;
    }>;
};
/**
 * Handle an ask_user_question tool call.
 *
 * Flow:
 * 1. POST to /api/question/request — registers the question, backend emits Tauri event
 * 2. GET /api/question/await/:request_id — blocks until user answers
 * 3. Return the answer JSON to the agent
 */
export declare function handleAskUserQuestion(args: AskUserQuestionArgs): Promise<ToolTextResult>;
/**
 * Handle a plan-mode proposal tool call.
 *
 * This uses the same pending-question transport as ask_user_question so the
 * UI can show a confirmation card and the agent receives a blocking result.
 */
export declare function handleProposePlanMode(args: ProposePlanModeArgs): Promise<ToolTextResult>;
export {};
//# sourceMappingURL=question-handler.d.ts.map