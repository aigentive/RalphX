import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useQuestionInput } from "./useQuestionInput";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";

const question: AskUserQuestionPayload = {
  requestId: "req-1",
  sessionId: "session-1",
  question: "Which database should we use?",
  options: [
    { value: "pg", label: "PostgreSQL" },
    { value: "sqlite", label: "SQLite" },
  ],
  multiSelect: false,
};

describe("useQuestionInput", () => {
  it("sends a normal chat message when a late answer is not delivered to a waiting agent", async () => {
    const submitAnswer = vi.fn().mockResolvedValue({
      success: true,
      deliveredToWaitingAgent: false,
    });
    const handleSend = vi.fn().mockResolvedValue(undefined);

    const { result } = renderHook(() =>
      useQuestionInput({
        activeQuestion: question,
        submitAnswer,
        handleSend,
      })
    );

    act(() => {
      result.current.handleChipClick(0);
    });
    await act(async () => {
      await result.current.handleQuestionSend("");
    });

    expect(submitAnswer).toHaveBeenCalledWith(
      expect.objectContaining({
        requestId: "req-1",
        selectedOptions: ["pg"],
      })
    );
    expect(handleSend).toHaveBeenCalledWith(
      [
        "Answer to previous clarification question:",
        "Question: Which database should we use?",
        "Answer: PostgreSQL",
      ].join("\n")
    );
  });

  it("does not send a normal chat message when the answer reaches a waiting agent", async () => {
    const submitAnswer = vi.fn().mockResolvedValue({
      success: true,
      deliveredToWaitingAgent: true,
    });
    const handleSend = vi.fn().mockResolvedValue(undefined);

    const { result } = renderHook(() =>
      useQuestionInput({
        activeQuestion: question,
        submitAnswer,
        handleSend,
      })
    );

    act(() => {
      result.current.handleChipClick(1);
    });
    await act(async () => {
      await result.current.handleQuestionSend("");
    });

    expect(submitAnswer).toHaveBeenCalledWith(
      expect.objectContaining({
        requestId: "req-1",
        selectedOptions: ["sqlite"],
      })
    );
    expect(handleSend).not.toHaveBeenCalled();
  });

  it("submits a single option directly for inline action buttons", async () => {
    const submitAnswer = vi.fn().mockResolvedValue({
      success: true,
      deliveredToWaitingAgent: true,
    });
    const handleSend = vi.fn().mockResolvedValue(undefined);

    const { result } = renderHook(() =>
      useQuestionInput({
        activeQuestion: question,
        submitAnswer,
        handleSend,
      })
    );

    await act(async () => {
      await result.current.handleQuestionOptionSubmit(0);
    });

    expect(submitAnswer).toHaveBeenCalledWith({
      requestId: "req-1",
      taskId: undefined,
      selectedOptions: ["pg"],
    });
    expect(handleSend).not.toHaveBeenCalled();
  });

  it("submits a skipped response", async () => {
    const submitAnswer = vi.fn().mockResolvedValue({
      success: true,
      deliveredToWaitingAgent: true,
    });
    const handleSend = vi.fn().mockResolvedValue(undefined);

    const { result } = renderHook(() =>
      useQuestionInput({
        activeQuestion: question,
        submitAnswer,
        handleSend,
      })
    );

    await act(async () => {
      await result.current.handleQuestionSkip();
    });

    expect(submitAnswer).toHaveBeenCalledWith({
      requestId: "req-1",
      taskId: undefined,
      selectedOptions: [],
      skipped: true,
    });
    expect(handleSend).not.toHaveBeenCalled();
  });

  it("sends a late skipped answer as normal chat when the waiting agent is gone", async () => {
    const submitAnswer = vi.fn().mockResolvedValue({
      success: true,
      deliveredToWaitingAgent: false,
    });
    const handleSend = vi.fn().mockResolvedValue(undefined);

    const { result } = renderHook(() =>
      useQuestionInput({
        activeQuestion: question,
        submitAnswer,
        handleSend,
      })
    );

    await act(async () => {
      await result.current.handleQuestionSkip();
    });

    expect(handleSend).toHaveBeenCalledWith(
      [
        "Answer to previous clarification question:",
        "Question: Which database should we use?",
        "Answer: Skipped",
      ].join("\n")
    );
  });
});
