/**
 * useFreshnessBlockedNotification - Global toast for freshness-blocked task events.
 *
 * Listens for tasks transitioning to "blocked" status and shows an actionable
 * toast when the blocked reason is a FRESHNESS_BLOCKED structured message.
 * Primary action: "Reset & Retry" moves the task back to ready state.
 */

import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useEventBus } from "@/providers/EventProvider";
import { api } from "@/lib/tauri";
import { taskKeys } from "@/hooks/useTasks";
import { executionKeys } from "@/hooks/useExecutionControl";
import { FRESHNESS_BLOCKED_PREFIX, parseFreshnessBlockedReason } from "@/lib/freshness-blocked";
import { resumeExecutionIfStopped } from "@/lib/task-actions/resume-execution-if-stopped";
import { useAgentGate } from "@/hooks/useAgentGate";
import type { Unsubscribe } from "@/lib/event-bus";

// Minimum ms between toast notifications for the same task to avoid duplicates
const NOTIFICATION_COOLDOWN_MS = 60_000;

export function useFreshnessBlockedNotification() {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const executionResumeGate = useAgentGate("executionResume");
  // Map<taskId, lastNotifiedAt> — prevents duplicate toasts per session
  const notifiedRef = useRef(new Map<string, number>());

  useEffect(() => {
    const unsubscribes: Unsubscribe[] = [];

    // Listen for legacy task:status_changed events (simpler payload, always fired)
    unsubscribes.push(
      bus.subscribe<{
        task_id: string;
        old_status: string;
        new_status: string;
      }>("task:status_changed", async (payload) => {
        if (payload.new_status !== "blocked") return;

        const taskId = payload.task_id;
        const now = Date.now();
        const lastNotified = notifiedRef.current.get(taskId) ?? 0;
        if (now - lastNotified < NOTIFICATION_COOLDOWN_MS) return;

        try {
          const task = await api.tasks.get(taskId);
          if (!task.blockedReason?.startsWith(FRESHNESS_BLOCKED_PREFIX)) return;

          const parsed = parseFreshnessBlockedReason(task.blockedReason);
          if (!parsed) return;

          notifiedRef.current.set(taskId, now);

          const filesSummary =
            parsed.conflictFiles.length === 0
              ? "no conflict files recorded"
              : parsed.conflictFiles.length <= 3
                ? parsed.conflictFiles.join(", ")
                : `${parsed.conflictFiles.slice(0, 3).join(", ")} +${parsed.conflictFiles.length - 3} more`;

          const elapsedLabel =
            parsed.elapsedMinutes > 0 ? `${parsed.elapsedMinutes} min` : "unknown duration";

          toast.warning(`Branch freshness blocked — ${task.title}`, {
            description: `${parsed.totalAttempts} attempts over ${elapsedLabel}. Conflicts: ${filesSummary}`,
            duration: 20_000,
            action: {
              label: "Reset & Retry",
              onClick: async () => {
                try {
                  await api.tasks.move(taskId, "ready");
                  if (!executionResumeGate.gated) {
                    await resumeExecutionIfStopped(task.projectId);
                  }
                  // Allow re-notification after a manual reset
                  notifiedRef.current.delete(taskId);
                } catch {
                  toast.error("Failed to reset task — please try again from the task detail view");
                } finally {
                  queryClient.invalidateQueries({ queryKey: taskKeys.all });
                  queryClient.invalidateQueries({ queryKey: executionKeys.all });
                }
              },
            },
          });
        } catch {
          // Non-fatal: failed to fetch task for freshness notification
        }
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, executionResumeGate.gated, queryClient]);
}
