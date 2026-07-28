/**
 * useQuestionInput — manages chip selection, input value sync, and question-aware send
 *
 * Shared by embedded chat hosts to keep question handling isolated.
 * Handles selectedOptions state, chip click logic (single/multi-select),
 * onMatchedOptions callback, controlled input value, and question-aware send.
 */

import { useState, useEffect, useCallback } from "react";
import type { AskUserQuestionPayload, AskUserQuestionResponse } from "@/types/ask-user-question";
import type { SubmitQuestionAnswerResult } from "@/hooks/useAskUserQuestion";
import { useAgentGate } from "@/hooks/useAgentGate";

type QuestionSubmitResult = boolean | SubmitQuestionAnswerResult;

export interface UseQuestionInputParams {
  activeQuestion: AskUserQuestionPayload | null;
  submitAnswer: (response: AskUserQuestionResponse) => Promise<QuestionSubmitResult>;
  handleSend: (text: string) => Promise<void>;
}

function normalizeSubmitResult(result: QuestionSubmitResult): SubmitQuestionAnswerResult {
  if (typeof result === "boolean") {
    return { success: result, deliveredToWaitingAgent: true };
  }
  return result;
}

function formatLateQuestionAnswer(
  question: AskUserQuestionPayload,
  response: AskUserQuestionResponse
): string {
  const selectedLabels = response.selectedOptions.map((selected) => (
    question.options.find((option) => option.value === selected)?.label ?? selected
  ));
  const answer = response.skipped === true
    ? "Skipped"
    : selectedLabels.length > 0
      ? selectedLabels.join(", ")
      : response.customResponse?.trim() ?? "";

  return [
    "Answer to previous clarification question:",
    `Question: ${question.question}`,
    `Answer: ${answer}`,
  ].join("\n");
}

export function useQuestionInput({
  activeQuestion,
  submitAnswer,
  handleSend,
}: UseQuestionInputParams) {
  const agentGate = useAgentGate();
  const [selectedOptions, setSelectedOptions] = useState<Set<number>>(new Set());
  const [questionInputValue, setQuestionInputValue] = useState("");

  // Reset selection when question changes
  useEffect(() => {
    setSelectedOptions(new Set());
    setQuestionInputValue("");
  }, [activeQuestion?.requestId]);

  // Handle chip click → update selection + sync to input
  const handleChipClick = useCallback(
    (index: number) => {
      if (!activeQuestion) return;
      setSelectedOptions((prev: Set<number>) => {
        const next = new Set(prev);
        if (activeQuestion.multiSelect) {
          if (next.has(index)) next.delete(index);
          else next.add(index);
        } else {
          if (next.has(index)) next.clear();
          else { next.clear(); next.add(index); }
        }
        // Sync input value to show selected option labels
        const labels = Array.from(next)
          .sort()
          .map((i) => String(i + 1));
        setQuestionInputValue(labels.join(", "));
        return next;
      });
    },
    [activeQuestion]
  );

  // onMatchedOptions callback — called by ChatInput when user types numbers
  const handleMatchedOptions = useCallback((indices: number[]) => {
    setSelectedOptions(new Set(indices));
  }, []);

  // Question-aware send: if question active, build response and submitAnswer
  const handleQuestionSend = useCallback(
    async (text: string) => {
      // Answering a pending question is steering: the answer is what unblocks the
      // agent's next turn. Gated for a device without `ui:agent` (2.6-b). The plain
      // `handleSend` fall-through is gated by ChatInput's own check.
      if (agentGate.gated) return;

      if (!activeQuestion) {
        await handleSend(text);
        return;
      }

      const response: AskUserQuestionResponse = {
        requestId: activeQuestion.requestId,
        taskId: activeQuestion.taskId,
        selectedOptions: [],
      };

      if (selectedOptions.size > 0) {
        response.selectedOptions = Array.from(selectedOptions)
          .sort()
          .map((i) => activeQuestion.options[i]?.value ?? activeQuestion.options[i]?.label ?? "");
      } else if (text.trim()) {
        response.customResponse = text.trim();
      } else {
        return; // Nothing to submit
      }

      const submitResult = normalizeSubmitResult(await submitAnswer(response));
      const success = submitResult.success;
      if (success) {
        setSelectedOptions(new Set());
        setQuestionInputValue("");
        if (!submitResult.deliveredToWaitingAgent) {
          await handleSend(formatLateQuestionAnswer(activeQuestion, response));
        }
      }
    },
    [activeQuestion, agentGate.gated, selectedOptions, submitAnswer, handleSend]
  );

  const handleQuestionSkip = useCallback(
    async () => {
      // Skipping still submits an answer that releases the agent's next turn.
      if (agentGate.gated || !activeQuestion) return;

      const response: AskUserQuestionResponse = {
        requestId: activeQuestion.requestId,
        taskId: activeQuestion.taskId,
        selectedOptions: [],
        skipped: true,
      };

      const submitResult = normalizeSubmitResult(await submitAnswer(response));
      if (submitResult.success) {
        setSelectedOptions(new Set());
        setQuestionInputValue("");
        if (!submitResult.deliveredToWaitingAgent) {
          await handleSend(formatLateQuestionAnswer(activeQuestion, response));
        }
      }
    },
    [activeQuestion, agentGate.gated, submitAnswer, handleSend]
  );

  const handleQuestionOptionSubmit = useCallback(
    async (index: number) => {
      if (agentGate.gated || !activeQuestion) return;

      const option = activeQuestion.options[index];
      if (!option) return;

      const response: AskUserQuestionResponse = {
        requestId: activeQuestion.requestId,
        taskId: activeQuestion.taskId,
        selectedOptions: [option.value ?? option.label ?? ""],
      };

      const submitResult = normalizeSubmitResult(await submitAnswer(response));
      if (submitResult.success) {
        setSelectedOptions(new Set());
        setQuestionInputValue("");
        if (!submitResult.deliveredToWaitingAgent) {
          await handleSend(formatLateQuestionAnswer(activeQuestion, response));
        }
      }
    },
    [activeQuestion, agentGate.gated, submitAnswer, handleSend]
  );

  return {
    agentGate,
    selectedOptions,
    questionInputValue,
    setQuestionInputValue,
    handleChipClick,
    handleMatchedOptions,
    handleQuestionSend,
    handleQuestionSkip,
    handleQuestionOptionSubmit,
  };
}
