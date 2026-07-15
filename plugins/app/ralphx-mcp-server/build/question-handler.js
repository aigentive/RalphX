/**
 * Handler for ask_user_question MCP tool
 *
 * Mirrors the permission_request pattern:
 * 1. POST /api/question/request — registers question, emits Tauri event
 * 2. GET /api/question/await/:request_id — long-polls for user answer (about 5 min timeout)
 * 3. Returns answer to agent as tool result
 */
import { createHumanWaitAbortController, HUMAN_WAIT_CLIENT_TIMEOUT_MS, isHumanWaitTimeoutError, } from "./human-wait.js";
import { safeError } from "./redact.js";
import { buildTauriApiUrl } from "./tauri-client.js";
function errorResponse(message) {
    return {
        content: [
            {
                type: "text",
                text: JSON.stringify({
                    error: true,
                    message,
                }),
            },
        ],
    };
}
function questionTimeoutResponse() {
    return errorResponse("Question timed out waiting for user response. The user may be away. You can continue without the answer or try asking again later.");
}
function normalizeQuestionPrompts(args) {
    if (Array.isArray(args.questions) && args.questions.length > 0) {
        const prompts = [];
        for (const [index, question] of args.questions.entries()) {
            const text = question.question?.trim();
            if (!text) {
                return {
                    error: `Question ${index + 1} in questions[] is missing a question.`,
                };
            }
            prompts.push({
                id: question.id,
                question: text,
                header: question.header,
                options: question.options ?? [],
                multi_select: question.multi_select ?? false,
                allow_skip: question.allow_skip ?? args.allow_skip ?? true,
            });
        }
        return { prompts, isBatch: true };
    }
    const text = args.question?.trim();
    if (!text) {
        return {
            error: "ask_user_question requires either question or a non-empty questions[] array.",
        };
    }
    return {
        prompts: [
            {
                question: text,
                header: args.header,
                options: args.options ?? [],
                multi_select: args.multi_select ?? false,
                allow_skip: args.allow_skip ?? true,
            },
        ],
        isBatch: false,
    };
}
async function registerQuestion(sessionId, prompt, batchIndex, batchTotal, metadata) {
    const registerResponse = await fetch(buildTauriApiUrl("question/request"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
            session_id: sessionId,
            question: prompt.question,
            header: prompt.header,
            options: prompt.options.map((o) => ({
                value: o.value ?? o.label,
                label: o.label,
                description: o.description,
            })),
            multi_select: prompt.multi_select,
            allow_skip: prompt.allow_skip,
            batch_index: batchIndex,
            batch_total: batchTotal,
            ...(metadata !== undefined ? { metadata } : {}),
        }),
    });
    if (!registerResponse.ok) {
        const errorText = await registerResponse
            .text()
            .catch(() => registerResponse.statusText);
        throw new Error(`Failed to register question: ${errorText}`);
    }
    const result = (await registerResponse.json());
    return result.request_id;
}
function buildQuestionAnswerRecord(prompt, answer, requestId, index) {
    return {
        id: prompt.id ?? String(index + 1),
        request_id: requestId,
        header: prompt.header ?? null,
        question: prompt.question,
        options: prompt.options.map((option) => ({
            label: option.label,
            value: option.value ?? option.label,
            ...(option.description !== undefined ? { description: option.description } : {}),
        })),
        selected_options: answer.selected_options ?? [],
        text: answer.text ?? null,
        skipped: answer.skipped ?? false,
    };
}
async function askSingleQuestion(sessionId, prompt, batchIndex, batchTotal, metadata) {
    const requestId = await registerQuestion(sessionId, prompt, batchIndex, batchTotal, metadata);
    safeError(`[RalphX MCP] Question registered: ${requestId}`);
    // Keep our timeout just below the effective MCP tool ceiling so this path
    // returns structured timeout JSON instead of surfacing a raw transport error.
    const { controller, timeoutId } = createHumanWaitAbortController();
    const waitStartedAt = Date.now();
    try {
        const answerResponse = await fetch(buildTauriApiUrl(`question/await/${encodeURIComponent(requestId)}`), {
            method: "GET",
            signal: controller.signal,
        });
        clearTimeout(timeoutId);
        if (!answerResponse.ok) {
            if (answerResponse.status === 408) {
                safeError(`[RalphX MCP] Question ${requestId} timed out (backend)`);
                return { ok: false, response: questionTimeoutResponse() };
            }
            const errorText = await answerResponse
                .text()
                .catch(() => answerResponse.statusText);
            throw new Error(`Question await error: ${errorText}`);
        }
        const answer = (await answerResponse.json());
        safeError(`[RalphX MCP] Question ${requestId} answered`);
        return {
            ok: true,
            answer,
            requestId,
        };
    }
    catch (error) {
        clearTimeout(timeoutId);
        const elapsedMs = Date.now() - waitStartedAt;
        if (isHumanWaitTimeoutError(error, elapsedMs, HUMAN_WAIT_CLIENT_TIMEOUT_MS)) {
            safeError(`[RalphX MCP] Question ${requestId} timed out (client/transport)`);
            return { ok: false, response: questionTimeoutResponse() };
        }
        safeError(`[RalphX MCP] Question await error:`, error);
        throw error;
    }
}
/**
 * Handle an ask_user_question tool call.
 *
 * Flow:
 * 1. POST to /api/question/request — registers the question, backend emits Tauri event
 * 2. GET /api/question/await/:request_id — blocks until user answers
 * 3. Return the answer JSON to the agent
 */
export async function handleAskUserQuestion(args) {
    let sessionId;
    try {
        sessionId = currentWorkspaceConversationId({
            conversation_id: args.session_id,
        });
    }
    catch {
        return errorResponse("ask_user_question requires session_id because RalphX did not provide the current conversation id to the MCP runtime context.");
    }
    safeError(`[RalphX MCP] ask_user_question for session: ${sessionId}`);
    const normalized = normalizeQuestionPrompts(args);
    if ("error" in normalized) {
        return errorResponse(normalized.error);
    }
    const { prompts, isBatch } = normalized;
    const answers = [];
    for (const [index, prompt] of prompts.entries()) {
        try {
            const result = await askSingleQuestion(sessionId, prompt, isBatch ? index + 1 : undefined, isBatch ? prompts.length : undefined, args.metadata);
            if (!result.ok) {
                return result.response;
            }
            if (!isBatch) {
                const answerRecord = buildQuestionAnswerRecord(prompt, result.answer, result.requestId, index);
                return {
                    content: [
                        {
                            type: "text",
                            text: JSON.stringify({
                                selected_options: result.answer.selected_options ?? [],
                                text: result.answer.text ?? null,
                                skipped: result.answer.skipped ?? false,
                                answers: [answerRecord],
                            }),
                        },
                    ],
                };
            }
            answers.push(buildQuestionAnswerRecord(prompt, result.answer, result.requestId, index));
        }
        catch (error) {
            safeError(`[RalphX MCP] Failed to ask question:`, error);
            return {
                content: [
                    {
                        type: "text",
                        text: JSON.stringify({
                            error: true,
                            message: `Failed to ask question ${index + 1}: ${error instanceof Error ? error.message : String(error)}`,
                        }),
                    },
                ],
            };
        }
    }
    return {
        content: [
            {
                type: "text",
                text: JSON.stringify({ answers }),
            },
        ],
    };
}
function currentWorkspaceConversationId(args) {
    const explicit = args.conversation_id?.trim();
    if (explicit) {
        return explicit;
    }
    const runtimeConversationId = process.env.RALPHX_CONVERSATION_ID?.trim();
    if (runtimeConversationId) {
        return runtimeConversationId;
    }
    const parentConversationId = process.env.RALPHX_PARENT_CONVERSATION_ID?.trim();
    if (parentConversationId) {
        return parentConversationId;
    }
    const contextId = process.env.RALPHX_CONTEXT_ID?.trim();
    if (contextId) {
        return contextId;
    }
    throw new Error("propose_plan_mode requires conversation_id because RalphX did not provide the current conversation id to the MCP runtime context.");
}
/**
 * Handle a plan-mode proposal tool call.
 *
 * This uses the same pending-question transport as ask_user_question so the
 * UI can show a confirmation card and the agent receives a blocking result.
 */
export async function handleProposePlanMode(args) {
    try {
        const conversationId = currentWorkspaceConversationId(args);
        const reason = args.reason?.trim() || "The request would benefit from planning first.";
        const question = args.question?.trim() ||
            `${reason} Switch this conversation to Plan mode before continuing?`;
        const prompt = {
            question,
            header: "Switch to Plan mode?",
            options: [
                {
                    label: "Switch to Plan Mode",
                    value: "switch_to_plan",
                    description: "Use the planning workflow before execution.",
                },
            ],
            multi_select: false,
            allow_skip: true,
        };
        const metadata = {
            kind: "plan_mode_proposal",
            conversation_id: conversationId,
            current_mode: args.current_mode ?? null,
            reason,
        };
        const result = await askSingleQuestion(conversationId, prompt, undefined, undefined, metadata);
        if (!result.ok) {
            return result.response;
        }
        const selectedOptions = result.answer.selected_options ?? [];
        const skipped = result.answer.skipped ?? false;
        const accepted = !skipped && selectedOptions.includes("switch_to_plan");
        const status = accepted ? "accepted" : skipped ? "skipped" : "declined";
        return {
            content: [
                {
                    type: "text",
                    text: JSON.stringify({
                        type: "plan_mode_proposal",
                        request_id: result.requestId,
                        conversation_id: conversationId,
                        accepted,
                        status,
                        selected_options: selectedOptions,
                        text: result.answer.text ?? null,
                        skipped,
                    }),
                },
            ],
        };
    }
    catch (error) {
        safeError(`[RalphX MCP] Failed to propose plan mode:`, error);
        return {
            content: [
                {
                    type: "text",
                    text: JSON.stringify({
                        error: true,
                        message: `Failed to propose plan mode: ${error instanceof Error ? error.message : String(error)}`,
                    }),
                },
            ],
        };
    }
}
//# sourceMappingURL=question-handler.js.map