/**
 * Review event hooks - Tauri review event listeners with type-safe validation
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 */

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import { ReviewEventSchema } from "@/types/events";
import { reviewKeys } from "@/hooks/useReviews";

function normalizeReviewEventPayload(payload: unknown): unknown | null {
  if (!payload || typeof payload !== "object") {
    return payload;
  }

  const raw = payload as Record<string, unknown>;

  // Legacy/backend shape from EventEmitter.emit_with_payload:
  // { taskId: "...", payload: "{\"type\":\"started\",\"reviewId\":\"...\"}" }
  if ("payload" in raw) {
    const envelopeTaskId = typeof raw.taskId === "string" ? raw.taskId : undefined;
    const nestedPayload = raw.payload;

    if (typeof nestedPayload === "string") {
      try {
        const parsed = JSON.parse(nestedPayload) as Record<string, unknown>;
        // Ignore disabled review-start notifications (not actionable in frontend queries)
        if (parsed.type === "disabled") {
          return null;
        }
        if (envelopeTaskId && typeof parsed.taskId !== "string") {
          parsed.taskId = envelopeTaskId;
        }
        return parsed;
      } catch {
        return payload;
      }
    }

    if (nestedPayload && typeof nestedPayload === "object") {
      const parsed = { ...(nestedPayload as Record<string, unknown>) };
      if (parsed.type === "disabled") {
        return null;
      }
      if (envelopeTaskId && typeof parsed.taskId !== "string") {
        parsed.taskId = envelopeTaskId;
      }
      return parsed;
    }
  }

  return payload;
}

/**
 * Hook to listen for review events
 *
 * Listens to 'review:update' events and invalidates TanStack Query caches
 * to trigger refetching of review-related data.
 *
 * @example
 * ```tsx
 * function ReviewSummary() {
 *   useReviewEvents(); // Auto-refreshes review data on backend events
 *   const { data } = usePendingReviews(projectId);
 *   return <ReviewList reviews={data} />;
 * }
 * ```
 */
export function useReviewEvents() {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    return bus.subscribe<unknown>("review:update", (payload) => {
      const normalizedPayload = normalizeReviewEventPayload(payload);
      if (normalizedPayload === null) {
        return;
      }

      // Runtime validation of backend events
      const parsed = ReviewEventSchema.safeParse(normalizedPayload);

      if (!parsed.success) {
        console.error("Invalid review event:", parsed.error.message);
        return;
      }

      const reviewEvent = parsed.data;

      // Always invalidate pending reviews (all events affect this)
      queryClient.invalidateQueries({
        queryKey: reviewKeys.pending(),
      });

      // Keep legacy review task consumers in sync.
      queryClient.invalidateQueries({
        queryKey: reviewKeys.tasksAwaitingReview(),
      });

      // For completed events, also invalidate task-specific queries
      if (reviewEvent.type === "completed") {
        queryClient.invalidateQueries({
          queryKey: reviewKeys.byTaskId(reviewEvent.taskId),
        });
        queryClient.invalidateQueries({
          queryKey: reviewKeys.stateHistoryById(reviewEvent.taskId),
        });
      }
    });
  }, [bus, queryClient]);
}
