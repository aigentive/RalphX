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
    session_id: string;
    question?: string;
    header?: string;
    options?: QuestionOption[];
    multi_select?: boolean;
    allow_skip?: boolean;
    questions?: QuestionPrompt[];
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
export {};
//# sourceMappingURL=question-handler.d.ts.map