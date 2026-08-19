/**
 * useCloneJob — drives one project clone from start through terminal state.
 *
 * The backend event bus only bridges events that reach a registered listener,
 * so events alone cannot be trusted for terminal state (see
 * clone_job_registry.rs). This hook therefore reconciles via
 * `get_clone_job_status` immediately after start and on a bounded interval
 * while the job is non-terminal; the first terminal answer from either source
 * wins and stops the interval. Every handler rejects events/status reads for
 * a job id other than the currently active one.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { useEventBus } from "@/providers/EventProvider";
import { projectsApi, type StartProjectCloneInput } from "@/api/projects";
import {
  CloneCancelledEventSchema,
  CloneCompletedEventSchema,
  CloneFailedEventSchema,
  CloneProgressEventSchema,
  transformCloneJobStatus,
  type ClonePhase,
  type CloneJobStatus,
} from "@/types/clone";

const POLL_INTERVAL_MS = 2000;
const MAX_LINES = 200;
const UNKNOWN_ERROR_MESSAGE = "We lost track of this clone. Try again.";
const START_FAILED_MESSAGE = "Could not start the clone. Try again.";

export interface UseCloneJobResult {
  phase: ClonePhase | null;
  percent: number | null;
  received: number | null;
  total: number | null;
  lines: string[];
  status: CloneJobStatus | null;
  error: string | null;
  cleanedUp: boolean | null;
  start: (input: StartProjectCloneInput) => Promise<void>;
  cancel: () => Promise<void>;
  reset: () => void;
}

export function useCloneJob(): UseCloneJobResult {
  const bus = useEventBus();
  const activeJobIdRef = useRef<string | null>(null);
  const settledRef = useRef(false);
  const pollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const [phase, setPhase] = useState<ClonePhase | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  const [received, setReceived] = useState<number | null>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState<CloneJobStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cleanedUp, setCleanedUp] = useState<boolean | null>(null);

  const stopPolling = useCallback(() => {
    if (pollTimerRef.current !== null) {
      clearInterval(pollTimerRef.current);
      pollTimerRef.current = null;
    }
  }, []);

  const clearJobState = useCallback(() => {
    stopPolling();
    settledRef.current = false;
    activeJobIdRef.current = null;
    setPhase(null);
    setPercent(null);
    setReceived(null);
    setTotal(null);
    setLines([]);
    setStatus(null);
    setError(null);
    setCleanedUp(null);
  }, [stopPolling]);

  // First terminal answer (event or status read) wins; later ones are ignored.
  const settleTerminal = useCallback(
    (jobId: string, next: CloneJobStatus) => {
      if (jobId !== activeJobIdRef.current || settledRef.current) {
        return;
      }
      settledRef.current = true;
      stopPolling();
      setStatus(next);
      switch (next.state) {
        case "failed":
          setError(next.message);
          setCleanedUp(next.cleanedUp);
          break;
        case "cancelled":
          setError(null);
          setCleanedUp(next.cleanedUp);
          break;
        case "unknown":
          setError(UNKNOWN_ERROR_MESSAGE);
          setCleanedUp(null);
          break;
        case "completed":
          setError(null);
          setCleanedUp(null);
          break;
        default:
          break;
      }
    },
    [stopPolling]
  );

  const pollStatus = useCallback(
    async (jobId: string) => {
      if (jobId !== activeJobIdRef.current || settledRef.current) {
        return;
      }
      let next: CloneJobStatus;
      try {
        next = await projectsApi.getCloneJobStatus(jobId);
      } catch {
        // Transient IPC failure — the next interval tick (or the still-live
        // event subscription) gets another chance; this is not terminal.
        return;
      }
      if (jobId !== activeJobIdRef.current || settledRef.current) {
        return;
      }
      if (next.state === "running") {
        setStatus(next);
        setPhase(next.phase);
        setPercent(next.percent);
        return;
      }
      settleTerminal(jobId, next);
    },
    [settleTerminal]
  );

  useEffect(() => {
    const unsubProgress = bus.subscribe<unknown>("project:clone_progress", (payload) => {
      const parsed = CloneProgressEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }
      const event = parsed.data;
      if (event.jobId !== activeJobIdRef.current || settledRef.current) {
        return;
      }
      setPhase(event.phase);
      setPercent(event.percent);
      setReceived(event.received);
      setTotal(event.total);
      setLines((prev) => {
        const nextLines = [...prev, event.line];
        return nextLines.length > MAX_LINES
          ? nextLines.slice(nextLines.length - MAX_LINES)
          : nextLines;
      });
    });

    const unsubCompleted = bus.subscribe<unknown>("project:clone_completed", (payload) => {
      const parsed = CloneCompletedEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }
      settleTerminal(parsed.data.jobId, transformCloneJobStatus(parsed.data));
    });

    const unsubFailed = bus.subscribe<unknown>("project:clone_failed", (payload) => {
      const parsed = CloneFailedEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }
      settleTerminal(parsed.data.jobId, transformCloneJobStatus(parsed.data));
    });

    const unsubCancelled = bus.subscribe<unknown>("project:clone_cancelled", (payload) => {
      const parsed = CloneCancelledEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }
      settleTerminal(parsed.data.jobId, transformCloneJobStatus(parsed.data));
    });

    return () => {
      unsubProgress();
      unsubCompleted();
      unsubFailed();
      unsubCancelled();
      stopPolling();
    };
  }, [bus, settleTerminal, stopPolling]);

  const start = useCallback(
    async (input: StartProjectCloneInput) => {
      clearJobState();

      let jobId: string;
      try {
        const response = await projectsApi.startProjectClone(input);
        jobId = response.jobId;
      } catch (invokeError) {
        setError(invokeError instanceof Error ? invokeError.message : START_FAILED_MESSAGE);
        return;
      }

      activeJobIdRef.current = jobId;
      void pollStatus(jobId);
      pollTimerRef.current = setInterval(() => {
        void pollStatus(jobId);
      }, POLL_INTERVAL_MS);
    },
    [clearJobState, pollStatus]
  );

  const cancel = useCallback(async () => {
    const jobId = activeJobIdRef.current;
    if (jobId === null) {
      return;
    }
    await projectsApi.cancelProjectClone(jobId);
  }, []);

  return { phase, percent, received, total, lines, status, error, cleanedUp, start, cancel, reset: clearJobState };
}
