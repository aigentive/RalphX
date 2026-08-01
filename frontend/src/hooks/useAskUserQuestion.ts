/**
 * useAskUserQuestion hook - Handle agent questions requiring user input
 *
 * Listens for agent:ask_user_question Tauri events, stores per-session
 * question payloads in uiStore, and provides functions to submit answers
 * or dismiss questions.
 */

import { useEffect, useState, useCallback, useRef } from "react";
import { z } from "zod";
import { toast } from "sonner";
import { useEventBus } from "@/providers/EventProvider";
import { api } from "@/lib/tauri";
import { useUiStore } from "@/stores/uiStore";
import { useEnvironmentStore } from "@/stores/environmentStore";
import {
  onPendingGateReconcile,
} from "@/lib/remote/pending-gate-reconcile";
import { remoteErrorBannerProps } from "@/lib/remote/agent-gate";
import { isRemoteTransportError } from "@/lib/remote/transport-errors";
import {
  AskUserQuestionPayloadSchema,
  type AskUserQuestionResponse,
} from "@/types/ask-user-question";

const QuestionResolvedPayloadSchema = z.object({
  sessionId: z.string().min(1),
  requestId: z.string().min(1),
});

export interface SubmitQuestionAnswerResult {
  success: boolean;
  deliveredToWaitingAgent: boolean;
  planModeProposalHandled?: boolean;
}

/**
 * Module-level map of recently answered requestIds → timestamp.
 * Used as a hydration guard to prevent resolved questions from reappearing
 * on mount. TTL: 5 minutes.
 */
const answeredRequestIds = new Map<string, number>();
const ANSWERED_TTL_MS = 5 * 60 * 1000;

function pruneAnsweredRequestIds() {
  const cutoff = Date.now() - ANSWERED_TTL_MS;
  for (const [id, ts] of answeredRequestIds) {
    if (ts < cutoff) answeredRequestIds.delete(id);
  }
}

/**
 * Hook to handle agent questions requiring user input, scoped to a session.
 *
 * @param currentSessionId - The session/conversation ID to scope questions to.
 *   When undefined, no question is returned (but events are still stored).
 */
export function useAskUserQuestion(currentSessionId: string | undefined) {
  const [isLoading, setIsLoading] = useState(false);
  const autoDismissTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const activeQuestion = useUiStore((s) =>
    currentSessionId ? (s.activeQuestions[currentSessionId] ?? null) : null
  );
  const answeredQuestion = useUiStore((s) =>
    currentSessionId ? (s.answeredQuestions[currentSessionId] ?? undefined) : undefined
  );

  const setActiveQuestion = useUiStore((s) => s.setActiveQuestion);
  const clearActiveQuestion = useUiStore((s) => s.clearActiveQuestion);
  const dismissQuestionAction = useUiStore((s) => s.dismissQuestion);
  const setAnsweredQuestion = useUiStore((s) => s.setAnsweredQuestion);
  const clearAnsweredQuestion = useUiStore((s) => s.clearAnsweredQuestion);
  const eventBus = useEventBus();

  /**
   * Cancel any pending auto-dismiss timer
   */
  const cancelAutoDismissTimer = useCallback(() => {
    if (autoDismissTimerRef.current) {
      clearTimeout(autoDismissTimerRef.current);
      autoDismissTimerRef.current = null;
    }
  }, []);

  // Clean up timer on unmount
  useEffect(() => {
    return () => {
      cancelAutoDismissTimer();
    };
  }, [cancelAutoDismissTimer]);

  // Hydrate on mount: fetch pending questions from backend in case the Tauri event was missed
  // (e.g., the panel wasn't mounted when the agent called ask_user_question).
  // Also clears records only after the backend says the question is no longer unresolved.
  useEffect(() => {
    if (!currentSessionId) return;

    // Snapshot requestId before async call for race detection
    const preCallRequestId = useUiStore.getState().activeQuestions[currentSessionId]?.requestId;

    api.askUserQuestion.getPendingQuestions().then((questions) => {
      const match = questions.find((q) => q.sessionId === currentSessionId);
      if (match) {
        // Skip hydration if this question was already answered in this session
        if (answeredRequestIds.has(match.requestId)) return;
        cancelAutoDismissTimer();
        setActiveQuestion(currentSessionId, match);
      } else {
        // Clear stale: backend says no pending question, but store still has one.
        // Only clear if requestId unchanged (an event didn't replace it during the call).
        const currentQuestion = useUiStore.getState().activeQuestions[currentSessionId];
        if (currentQuestion && currentQuestion.requestId === preCallRequestId) {
          clearActiveQuestion(currentSessionId);
        }
      }
    }).catch(() => {
      // Non-critical — event listener is the primary live delivery path
    });
  // Run once per session ID change — intentionally excludes activeQuestion to avoid loops
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentSessionId]);

  /**
   * P-21 (2.7-c): AUTHORITATIVE reconciliation on every (re)connect.
   *
   * A question gate raised while this client was disconnected produced no event we ever
   * saw, and one resolved while we were away produced none either. The visibility-based
   * reconcile below only ever CLEARS, and only when the app is refocused; a reconnect
   * that happens while the window is already visible reaches neither.
   *
   * FAIL CLOSED: the strict command raises rather than answering `[]`. On failure the
   * banner that is already up stays up and the user is told, because clearing a live
   * gate on an unreadable state is how an agent ends up waiting forever on an answer
   * nobody can see it needs.
   */
  useEffect(() => {
    if (!currentSessionId) return undefined;

    return onPendingGateReconcile(({ environmentId }) => {
      if (environmentId !== useEnvironmentStore.getState().activeEnvironmentId) {
        // SCOPE: a background environment's connect must not rewrite this banner.
        return;
      }
      const preCallRequestId =
        useUiStore.getState().activeQuestions[currentSessionId]?.requestId;

      api.askUserQuestion
        .listPendingQuestionGates()
        .then((questions) => {
          const match = questions.find((q) => q.sessionId === currentSessionId);
          if (match) {
            if (answeredRequestIds.has(match.requestId)) return;
            cancelAutoDismissTimer();
            setActiveQuestion(currentSessionId, match);
            return;
          }
          // Absent from the authoritative set = resolved or expired. Drop it, unless a
          // live event replaced it while this call was in flight.
          const current = useUiStore.getState().activeQuestions[currentSessionId];
          if (current && current.requestId === preCallRequestId) {
            clearActiveQuestion(currentSessionId);
          }
        })
        .catch((error: unknown) => {
          console.error("Failed to reconcile pending questions:", error);
          toast.error("Couldn't refresh the pending question");
        });
    });
  }, [
    cancelAutoDismissTimer,
    clearActiveQuestion,
    currentSessionId,
    setActiveQuestion,
  ]);

  // Reconcile on window focus: detect resolved/removed questions when returning to the app.
  useEffect(() => {
    if (!currentSessionId) return undefined;

    let debounceTimer: ReturnType<typeof setTimeout> | null = null;

    function handleVisibilityChange() {
      if (document.visibilityState !== "visible") return;

      // Only check if there's an active question for this session
      const questionForSession = useUiStore.getState().activeQuestions[currentSessionId!];
      if (!questionForSession) return;

      const preCallRequestId = questionForSession.requestId;

      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        api.askUserQuestion.getPendingQuestions().then((pending) => {
          const stillPending = pending.some((q) => q.sessionId === currentSessionId);
          if (!stillPending) {
            // Verify question wasn't replaced by a new event during the API call
            const current = useUiStore.getState().activeQuestions[currentSessionId!];
            if (current && current.requestId === preCallRequestId) {
              clearActiveQuestion(currentSessionId!);
            }
          }
        }).catch(() => {
          // Non-critical — don't disrupt UX
        });
      }, 500);
    }

    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  }, [currentSessionId, clearActiveQuestion]);

  // Set up event listener for agent questions — stores ALL incoming questions by sessionId
  useEffect(() => {
    const unsubscribe = eventBus.subscribe<unknown>("agent:ask_user_question", (payload) => {
      const parsed = AskUserQuestionPayloadSchema.safeParse(payload);

      if (!parsed.success) {
        console.warn("[useAskUserQuestion] Zod parse failed:", parsed.error.issues);
        return;
      }

      const sessionId = parsed.data.sessionId;
      if (!sessionId) {
        console.warn("[useAskUserQuestion] No sessionId in payload, ignoring");
        return;
      }

      // Cancel any pending auto-dismiss timer for this session (new question arrived)
      // Also clear stale answered state so the new question isn't hidden behind it
      if (sessionId === currentSessionId) {
        cancelAutoDismissTimer();
        clearAnsweredQuestion(sessionId);
      }

      setActiveQuestion(sessionId, parsed.data);
    });

    return unsubscribe;
  }, [setActiveQuestion, eventBus, currentSessionId, cancelAutoDismissTimer, clearAnsweredQuestion]);

  // Listen for backend-emitted question_resolved events (defense-in-depth cleanup).
  // Uses fresh store state to avoid stale closure, clears only if requestId matches.
  useEffect(() => {
    const unsubscribe = eventBus.subscribe<unknown>("agent:question_resolved", (payload) => {
      const parsed = QuestionResolvedPayloadSchema.safeParse(payload);
      if (!parsed.success) return;

      const { sessionId, requestId } = parsed.data;
      const fresh = useUiStore.getState().activeQuestions[sessionId];
      if (fresh && fresh.requestId === requestId) {
        clearActiveQuestion(sessionId);
      }
    });

    return unsubscribe;
  }, [eventBus, clearActiveQuestion]);

  /**
   * Submit an answer to the agent's question.
   * Routes to resolveQuestion (MCP flow) when requestId is present,
   * or answerQuestion (legacy task flow) otherwise.
   */
  const submitAnswer = useCallback(
    async (response: AskUserQuestionResponse): Promise<SubmitQuestionAnswerResult> => {
      if (!activeQuestion || !currentSessionId) {
        return { success: false, deliveredToWaitingAgent: false };
      }

      // Capture the requestId we're answering — a new question may arrive while we await.
      const submittedRequestId = activeQuestion.requestId;
      let deliveredToWaitingAgent = true;
      let planModeProposalHandled = false;

      setIsLoading(true);
      try {
        if (response.requestId) {
          const result = await api.askUserQuestion.resolveQuestion({
            requestId: response.requestId,
            selectedOptions: response.selectedOptions,
            ...(response.customResponse !== undefined && { customResponse: response.customResponse }),
            ...(response.skipped !== undefined && { skipped: response.skipped }),
          });
          deliveredToWaitingAgent = result?.deliveredToWaitingAgent ?? true;
          planModeProposalHandled = result?.planModeProposalHandled ?? false;
        } else {
          await api.askUserQuestion.answerQuestion(response);
        }

        // Only clear if the question hasn't been replaced by a new one while we were awaiting.
        const currentQuestion = useUiStore.getState().activeQuestions[currentSessionId];
        if (!currentQuestion || currentQuestion.requestId === submittedRequestId) {
          answeredRequestIds.set(submittedRequestId, Date.now());
          pruneAnsweredRequestIds();
          const summary = response.skipped === true
            ? "Skipped"
            : response.selectedOptions.length > 0
              ? response.selectedOptions.join(", ")
              : response.customResponse ?? "";
          setAnsweredQuestion(currentSessionId, summary);
          clearActiveQuestion(currentSessionId);

          // Auto-dismiss the answered banner after 3500ms
          cancelAutoDismissTimer();
          autoDismissTimerRef.current = setTimeout(() => {
            clearAnsweredQuestion(currentSessionId);
            autoDismissTimerRef.current = null;
          }, 3500);
        }
        return { success: true, deliveredToWaitingAgent, planModeProposalHandled };
      } catch (error: unknown) {
        const currentQuestion = useUiStore.getState().activeQuestions[currentSessionId];
        if (currentQuestion?.requestId !== submittedRequestId) {
          // Already replaced or cleared by an authoritative path — nothing of ours to write.
          return { success: false, deliveredToWaitingAgent: false };
        }

        // FAIL CLOSED (stateful-workflow-review): a TRANSPORT failure is not evidence about
        // the gate. `RemoteTransportError` is raised by the wrapper itself — a registered
        // command that ran on the host and returned `Err` rejects with that `Err`, never with
        // this type — so it says the request did not reach a verdict, and says nothing at all
        // about whether the agent is still blocked. Clearing the banner on it was a
        // false-terminal write: the host agent keeps waiting on an answer nobody can see it
        // needs, and the user is told a session expired that did not. Keep the banner up, keep
        // the requestId out of `answeredRequestIds` so reconcile can still restore it, surface
        // the failure, and leave retry possible.
        if (isRemoteTransportError(error)) {
          const banner = remoteErrorBannerProps(error);
          if (banner) {
            toast.error(banner.title, { description: banner.body, duration: 8000 });
          } else {
            toast.error("Couldn't send your answer", {
              description: "The question is still waiting — try again.",
              duration: 8000,
            });
          }
          return { success: false, deliveredToWaitingAgent: false };
        }

        // A host-produced `Err` IS authoritative about this gate (the host read its own
        // question state to answer). Preserve the existing local behaviour.
        toast.error("Agent session expired — question is no longer active", { duration: 5000 });
        clearActiveQuestion(currentSessionId);
        return { success: false, deliveredToWaitingAgent: false };
      } finally {
        setIsLoading(false);
      }
    },
    [activeQuestion, currentSessionId, clearActiveQuestion, setAnsweredQuestion, clearAnsweredQuestion, cancelAutoDismissTimer]
  );

  /**
   * Dismiss the question — clears both question and answered state for this session,
   * and sends a dismiss response to the backend so the waiting agent unblocks.
   */
  const dismissQuestion = useCallback(async () => {
    if (!currentSessionId) return;

    const question = activeQuestion;
    dismissQuestionAction(currentSessionId);

    // Cancel any pending auto-dismiss timer
    cancelAutoDismissTimer();

    // If there's an active question with a requestId, send dismiss to backend
    if (question?.requestId) {
      try {
        await api.askUserQuestion.resolveQuestion({
          requestId: question.requestId,
          selectedOptions: [],
          customResponse: "[dismissed]",
        });
        // Suppress re-hydration only AFTER the host confirmed the dismissal. Recording it
        // up front was a false-terminal write with a 5-minute blast radius: a transport
        // refusal left the gate live on the host while this client suppressed every
        // rehydration and reconcile of it for `ANSWERED_TTL_MS`, so the banner could not
        // come back and the agent waited on an answer nobody could see it needed.
        answeredRequestIds.set(question.requestId, Date.now());
        pruneAnsweredRequestIds();
      } catch (error: unknown) {
        if (!isRemoteTransportError(error)) {
          // A host-produced `Err` is authoritative about this gate — it is gone either way.
          answeredRequestIds.set(question.requestId, Date.now());
          pruneAnsweredRequestIds();
          return;
        }
        // Transport failure: the gate is untouched on the host. Put the banner back rather
        // than leaving a still-blocked agent invisible, and say why.
        setActiveQuestion(currentSessionId, question);
        const banner = remoteErrorBannerProps(error);
        if (banner) {
          toast.error(banner.title, { description: banner.body, duration: 8000 });
        } else {
          toast.error("Couldn't dismiss the question", {
            description: "The question is still waiting — try again.",
            duration: 8000,
          });
        }
      }
    }
  }, [
    currentSessionId,
    activeQuestion,
    dismissQuestionAction,
    cancelAutoDismissTimer,
    setActiveQuestion,
  ]);

  /**
   * Clear just the answered summary for this session
   */
  const clearAnswered = useCallback(() => {
    if (!currentSessionId) return;
    clearAnsweredQuestion(currentSessionId);
  }, [currentSessionId, clearAnsweredQuestion]);

  return {
    activeQuestion,
    answeredQuestion,
    submitAnswer,
    dismissQuestion,
    clearAnswered,
    isLoading,
  };
}
