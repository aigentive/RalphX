/**
 * Which observed frames count toward a background environment's notification badge
 * (§6.4, A-12).
 *
 * A background supervisor may OBSERVE to count; it may never PROJECT. This module is
 * deliberately pure and total: the decision of "does this frame move a badge" is the
 * only thing the background relay is allowed to compute from a frame, so keeping it
 * here — rather than inline in the relay closure — keeps the observation surface
 * auditable and makes the negative cases (transients, unrelated events, control
 * frames) testable without booting a runtime.
 */

import type { RemoteServerFrame } from "./stream-frames";

/**
 * The one durable event a badge counts.
 *
 * Mirrors `NOTIFICATION_CREATED_EVENT` in
 * `src-tauri/src/application/notification_service.rs`. `notification:updated` is
 * deliberately NOT counted: an update is a mutation of a notification the badge has
 * already tallied (or one it never saw), so counting it would inflate the badge
 * against the same underlying row.
 */
export const BADGE_COUNTED_EVENT_NAME = "notification:created";

/**
 * True when an observed frame should increment a background environment's badge.
 *
 * DURABLE ONLY. A transient frame carries no `seq` (§3.4), so it has no durable
 * identity to reconcile against hydrated notification state on reactivation — a
 * badge built from transients would double-count the moment the environment
 * cold-hydrates. Counting only sequenced frames means the tally is always a subset
 * of what the hydrate will re-derive.
 */
export function countsTowardBadge(frame: RemoteServerFrame): boolean {
  return (
    frame.type === "event" &&
    frame.seq !== null &&
    frame.name === BADGE_COUNTED_EVENT_NAME
  );
}
