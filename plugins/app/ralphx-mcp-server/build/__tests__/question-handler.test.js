import { afterEach, describe, expect, it, vi } from "vitest";
import { handleAskUserQuestion, handleProposePlanMode } from "../question-handler.js";
function jsonResponse(body, status = 200) {
    return new Response(JSON.stringify(body), {
        status,
        headers: { "Content-Type": "application/json" },
    });
}
function parsedToolText(result) {
    return JSON.parse(result.content[0]?.text ?? "{}");
}
describe("handleAskUserQuestion", () => {
    afterEach(() => {
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        delete process.env.RALPHX_CONVERSATION_ID;
        delete process.env.RALPHX_PARENT_CONVERSATION_ID;
        delete process.env.RALPHX_CONTEXT_ID;
    });
    it("keeps legacy single-question fields and adds a renderable answer record", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-1" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: ["yes"],
            text: null,
            skipped: false,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            header: "Launch gate",
            question: "Proceed?",
            options: [{ label: "Yes", value: "yes" }],
        });
        expect(parsedToolText(result)).toEqual({
            selected_options: ["yes"],
            text: null,
            skipped: false,
            answers: [
                {
                    id: "1",
                    request_id: "req-1",
                    header: "Launch gate",
                    question: "Proceed?",
                    options: [{ label: "Yes", value: "yes" }],
                    selected_options: ["yes"],
                    text: null,
                    skipped: false,
                },
            ],
        });
        const requestBody = JSON.parse(fetchMock.mock.calls[0]?.[1]?.body);
        expect(requestBody).toMatchObject({
            session_id: "session-1",
            question: "Proceed?",
            allow_skip: true,
            multi_select: false,
        });
        expect(requestBody.batch_index).toBeUndefined();
        expect(requestBody.batch_total).toBeUndefined();
    });
    it("uses the runtime conversation id and forwards metadata when session_id is omitted", async () => {
        process.env.RALPHX_CONVERSATION_ID = "conversation-automation";
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-automation" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: ["apply_automation_proposal"],
            text: null,
            skipped: false,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            header: "Apply automation proposal?",
            question: "Apply this automation proposal?",
            options: [{ label: "Apply proposal", value: "apply_automation_proposal" }],
            allow_skip: false,
            metadata: {
                kind: "automation_setup_proposal",
            },
        });
        expect(parsedToolText(result)).toMatchObject({
            selected_options: ["apply_automation_proposal"],
            skipped: false,
        });
        const requestBody = JSON.parse(fetchMock.mock.calls[0]?.[1]?.body);
        expect(requestBody).toMatchObject({
            session_id: "conversation-automation",
            header: "Apply automation proposal?",
            allow_skip: false,
            metadata: {
                kind: "automation_setup_proposal",
            },
        });
    });
    it("asks batched interview questions sequentially", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-1" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: ["backend"],
            text: null,
            skipped: false,
        }))
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-2" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: [],
            text: null,
            skipped: true,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            questions: [
                {
                    id: "scope",
                    question: "Which area should we focus on?",
                    options: [{ label: "Backend", value: "backend" }],
                },
                {
                    id: "deadline",
                    question: "Any deadline?",
                    allow_skip: true,
                },
            ],
        });
        expect(parsedToolText(result)).toEqual({
            answers: [
                {
                    id: "scope",
                    request_id: "req-1",
                    header: null,
                    question: "Which area should we focus on?",
                    options: [{ label: "Backend", value: "backend" }],
                    selected_options: ["backend"],
                    text: null,
                    skipped: false,
                },
                {
                    id: "deadline",
                    request_id: "req-2",
                    header: null,
                    question: "Any deadline?",
                    options: [],
                    selected_options: [],
                    text: null,
                    skipped: true,
                },
            ],
        });
        expect(fetchMock).toHaveBeenCalledTimes(4);
        expect(String(fetchMock.mock.calls[1]?.[0])).toContain("/api/question/await/req-1");
        expect(String(fetchMock.mock.calls[3]?.[0])).toContain("/api/question/await/req-2");
        const firstRequestBody = JSON.parse(fetchMock.mock.calls[0]?.[1]?.body);
        const secondRequestBody = JSON.parse(fetchMock.mock.calls[2]?.[1]?.body);
        expect(firstRequestBody).toMatchObject({
            batch_index: 1,
            batch_total: 2,
            allow_skip: true,
        });
        expect(secondRequestBody).toMatchObject({
            batch_index: 2,
            batch_total: 2,
            allow_skip: true,
        });
    });
    it("rejects calls without a single question or batch", async () => {
        const fetchMock = vi.fn();
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
        });
        expect(parsedToolText(result)).toEqual({
            error: true,
            message: "ask_user_question requires either question or a non-empty questions[] array.",
        });
        expect(fetchMock).not.toHaveBeenCalled();
    });
    it("rejects batched entries without question text", async () => {
        const fetchMock = vi.fn();
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            questions: [{ id: "empty", question: "   " }],
        });
        expect(parsedToolText(result)).toEqual({
            error: true,
            message: "Question 1 in questions[] is missing a question.",
        });
        expect(fetchMock).not.toHaveBeenCalled();
    });
    it("returns a structured error when registration fails", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(new Response("backend unavailable", { status: 500 }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            question: "Proceed?",
        });
        expect(parsedToolText(result)).toEqual({
            error: true,
            message: "Failed to ask question 1: Failed to register question: backend unavailable",
        });
    });
    it("returns the timeout payload when backend await expires", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-timeout" }))
            .mockResolvedValueOnce(new Response("timed out", { status: 408 }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            question: "Proceed?",
        });
        expect(parsedToolText(result)).toMatchObject({
            error: true,
            message: expect.stringContaining("Question timed out waiting for user response"),
        });
    });
    it("returns a structured error when backend await fails", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-error" }))
            .mockResolvedValueOnce(new Response("wait failed", { status: 500 }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            question: "Proceed?",
        });
        expect(parsedToolText(result)).toEqual({
            error: true,
            message: "Failed to ask question 1: Question await error: wait failed",
        });
    });
    it("returns the timeout payload when the transport aborts the wait", async () => {
        const abortError = new Error("The operation was aborted");
        abortError.name = "AbortError";
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-abort" }))
            .mockRejectedValueOnce(abortError);
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            question: "Proceed?",
        });
        expect(parsedToolText(result)).toMatchObject({
            error: true,
            message: expect.stringContaining("Question timed out waiting for user response"),
        });
    });
    it("returns a structured error when the await transport fails before timeout", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-network" }))
            .mockRejectedValueOnce(new Error("network down"));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleAskUserQuestion({
            session_id: "session-1",
            question: "Proceed?",
        });
        expect(parsedToolText(result)).toEqual({
            error: true,
            message: "Failed to ask question 1: network down",
        });
    });
});
describe("handleProposePlanMode", () => {
    afterEach(() => {
        vi.unstubAllGlobals();
        vi.restoreAllMocks();
        delete process.env.RALPHX_CONVERSATION_ID;
        delete process.env.RALPHX_PARENT_CONVERSATION_ID;
        delete process.env.RALPHX_CONTEXT_ID;
    });
    it("registers a plan-mode proposal question and returns accepted status", async () => {
        process.env.RALPHX_PARENT_CONVERSATION_ID = "conversation-1";
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-plan" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: ["switch_to_plan"],
            text: null,
            skipped: false,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleProposePlanMode({
            current_mode: "edit",
            reason: "This needs a structured requirements pass.",
        });
        expect(parsedToolText(result)).toEqual({
            type: "plan_mode_proposal",
            request_id: "req-plan",
            conversation_id: "conversation-1",
            accepted: true,
            status: "accepted",
            selected_options: ["switch_to_plan"],
            text: null,
            skipped: false,
        });
        const requestBody = JSON.parse(fetchMock.mock.calls[0]?.[1]?.body);
        expect(requestBody).toMatchObject({
            session_id: "conversation-1",
            header: "Switch to Plan mode?",
            allow_skip: true,
            multi_select: false,
            metadata: {
                kind: "plan_mode_proposal",
                conversation_id: "conversation-1",
                current_mode: "edit",
                reason: "This needs a structured requirements pass.",
            },
        });
        expect(requestBody.options).toEqual([
            {
                value: "switch_to_plan",
                label: "Switch to Plan Mode",
                description: "Use the planning workflow before execution.",
            },
        ]);
    });
    it("prefers the current MCP conversation id over the parent workspace conversation id", async () => {
        process.env.RALPHX_CONVERSATION_ID = "conversation-current";
        process.env.RALPHX_PARENT_CONVERSATION_ID = "conversation-parent";
        process.env.RALPHX_CONTEXT_ID = "project-1";
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-plan" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: ["switch_to_plan"],
            text: null,
            skipped: false,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleProposePlanMode({
            current_mode: "edit",
            reason: "This needs a structured requirements pass.",
        });
        expect(parsedToolText(result)).toMatchObject({
            type: "plan_mode_proposal",
            conversation_id: "conversation-current",
            accepted: true,
            status: "accepted",
        });
        const requestBody = JSON.parse(fetchMock.mock.calls[0]?.[1]?.body);
        expect(requestBody).toMatchObject({
            session_id: "conversation-current",
            metadata: {
                kind: "plan_mode_proposal",
                conversation_id: "conversation-current",
                current_mode: "edit",
            },
        });
    });
    it("returns skipped status without accepting plan mode", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-plan" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: [],
            text: null,
            skipped: true,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleProposePlanMode({
            conversation_id: "conversation-2",
            current_mode: "chat",
            reason: "This may need planning.",
        });
        expect(parsedToolText(result)).toMatchObject({
            type: "plan_mode_proposal",
            conversation_id: "conversation-2",
            accepted: false,
            status: "skipped",
            skipped: true,
        });
    });
    it("uses the MCP context id when explicit and parent ids are absent", async () => {
        process.env.RALPHX_CONTEXT_ID = "conversation-context";
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-plan" }))
            .mockResolvedValueOnce(jsonResponse({
            selected_options: [],
            text: "not yet",
            skipped: false,
        }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleProposePlanMode({
            current_mode: "edit",
            reason: "Needs planning.",
        });
        expect(parsedToolText(result)).toMatchObject({
            type: "plan_mode_proposal",
            conversation_id: "conversation-context",
            accepted: false,
            status: "declined",
            text: "not yet",
        });
    });
    it("returns a structured error when no conversation id is available", async () => {
        const fetchMock = vi.fn();
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleProposePlanMode({
            current_mode: "edit",
            reason: "Needs planning.",
        });
        expect(parsedToolText(result)).toEqual({
            error: true,
            message: "Failed to propose plan mode: propose_plan_mode requires conversation_id because RalphX did not provide the current conversation id to the MCP runtime context.",
        });
        expect(fetchMock).not.toHaveBeenCalled();
    });
    it("returns the timeout payload when proposal confirmation expires", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(jsonResponse({ request_id: "req-plan-timeout" }))
            .mockResolvedValueOnce(new Response("timed out", { status: 408 }));
        vi.stubGlobal("fetch", fetchMock);
        const result = await handleProposePlanMode({
            conversation_id: "conversation-3",
            current_mode: "edit",
            reason: "Needs planning.",
        });
        expect(parsedToolText(result)).toMatchObject({
            error: true,
            message: expect.stringContaining("Question timed out waiting for user response"),
        });
    });
});
//# sourceMappingURL=question-handler.test.js.map