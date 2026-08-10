import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

import { askUserQuestionApi } from "./ask-user-question";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";

describe("askUserQuestionApi", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  afterEach(() => {
    resetTransportEnvironmentId();
  });

  it("resolves MCP questions through the backend command", async () => {
    mockInvoke.mockResolvedValueOnce({
      success: true,
      message: null,
      deliveredToWaitingAgent: true,
      planModeProposalHandled: true,
    });

    await expect(
      askUserQuestionApi.resolveQuestion({
        requestId: "req-1",
        selectedOptions: ["approve"],
        customResponse: "Looks good",
      }),
    ).resolves.toEqual({
      success: true,
      message: null,
      deliveredToWaitingAgent: true,
      planModeProposalHandled: true,
    });

    expect(mockInvoke).toHaveBeenCalledWith("resolve_user_question", {
      args: {
        requestId: "req-1",
        selectedOptions: ["approve"],
        customResponse: "Looks good",
        skipped: false,
      },
    });
  });

  it("resolves remote MCP questions through the wrapped spawn-free twin", async () => {
    setTransportEnvironmentId("remote-1");
    const response = {
      success: true,
      message: "Question req-remote resolved",
      deliveredToWaitingAgent: false,
      planModeProposalHandled: false,
    };
    mockInvoke.mockResolvedValueOnce(response);

    await expect(
      askUserQuestionApi.resolveQuestion({
        requestId: "req-remote",
        selectedOptions: [],
        skipped: true,
      }),
    ).resolves.toEqual(response);

    expect(mockInvoke).toHaveBeenCalledWith("resolve_remote_user_question", {
      input: {
        requestId: "req-remote",
        selectedOptions: [],
        customResponse: undefined,
        skipped: true,
      },
    });
  });

  it("routes remote dismiss responses through the same wrapped twin", async () => {
    setTransportEnvironmentId("remote-1");
    mockInvoke.mockResolvedValueOnce({
      success: true,
      message: null,
      deliveredToWaitingAgent: true,
    });

    await askUserQuestionApi.resolveQuestion({
      requestId: "req-dismiss",
      selectedOptions: [],
      customResponse: "[dismissed]",
    });

    expect(mockInvoke).toHaveBeenCalledWith("resolve_remote_user_question", {
      input: {
        requestId: "req-dismiss",
        selectedOptions: [],
        customResponse: "[dismissed]",
        skipped: false,
      },
    });
  });

  it("maps pending question command results into UI payloads", async () => {
    mockInvoke.mockResolvedValueOnce([
      {
        request_id: "req-2",
        session_id: "session-1",
        question: "Which mode?",
        header: null,
        options: [{ value: "plan", label: "Plan" }],
        multi_select: false,
        allow_skip: null,
        batch_index: null,
        batch_total: 3,
        metadata: { source: "plan-mode" },
      },
    ]);

    await expect(askUserQuestionApi.getPendingQuestions()).resolves.toEqual([
      {
        requestId: "req-2",
        sessionId: "session-1",
        question: "Which mode?",
        header: null,
        options: [{ value: "plan", label: "Plan" }],
        multiSelect: false,
        allowSkip: true,
        batchIndex: null,
        batchTotal: 3,
        metadata: { source: "plan-mode" },
      },
    ]);
    expect(mockInvoke).toHaveBeenCalledWith("get_pending_questions");
  });

  it("answers legacy task questions through the backend command", async () => {
    mockInvoke.mockResolvedValueOnce(null);

    await askUserQuestionApi.answerQuestion({
      taskId: "task-1",
      selectedOptions: ["continue"],
      customResponse: "Proceed",
    });

    expect(mockInvoke).toHaveBeenCalledWith("answer_user_question", {
      input: {
        taskId: "task-1",
        selectedOptions: ["continue"],
        customResponse: "Proceed",
      },
    });
  });
});
