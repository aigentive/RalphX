import type { AttemptFailure } from "@/lib/remote/supervisor";
import type { EnvironmentConnectionState } from "@/stores/environmentStore";

interface EnvironmentStatusDotConfig {
  glyph: "●" | "◐" | "⊘" | "○";
  /** Never the plain green `●` unless the environment truly projects its stream. */
  color: string;
  reason: string | null;
}

export const ENVIRONMENT_STATUS_DOT = {
  idle: {
    glyph: "○",
    color: "var(--text-muted, #8e8e93)",
    reason: "Disconnected",
  },
  connecting: {
    glyph: "●",
    color: "var(--status-warning, #e8a33d)",
    reason: "Connecting…",
  },
  connected: {
    glyph: "●",
    color: "var(--status-success, #2eb867)",
    reason: null,
  },
  backoff: {
    glyph: "●",
    color: "var(--status-warning, #e8a33d)",
    reason: "Reconnecting…",
  },
  offline: {
    glyph: "○",
    color: "var(--text-muted, #8e8e93)",
    reason: "Disconnected",
  },
  blocked: {
    glyph: "⊘",
    color: "var(--status-error, #e5484d)",
    reason: "Blocked: protocol version",
  },
  suspended: {
    glyph: "◐",
    color: "var(--text-muted, #8e8e93)",
    reason: "Suspended",
  },
  // Reachable, live host — but no event stream is being projected for a background
  // environment, so it must never wear the green "connected" dot.
  health_only: {
    glyph: "◐",
    color: "var(--status-success, #2eb867)",
    reason: "Reachable in the background",
  },
} as const satisfies Record<EnvironmentConnectionState, EnvironmentStatusDotConfig>;

/** Why a blocked environment is blocked, in dot-tooltip form (2.7-a). */
const BLOCKED_REASON: Record<AttemptFailure, string> = {
  unauthorized: "Blocked: access revoked — re-pair",
  version: "Blocked: protocol version",
  malformed_descriptor: "Blocked: invalid host identity",
  invalid_request: "Blocked: update this app",
  // Never produced for `blocked` (a transient failure goes to backoff), but the map is
  // total so a future failure kind cannot silently fall back to the version copy.
  transient: "Blocked",
};

/**
 * The dot's tooltip. `blocked` alone does not say WHY, and the switcher used to assert
 * "protocol version" for every blocked cause — including a revoked device, whose fix is
 * re-pairing rather than updating.
 */
export function environmentStatusReason(
  state: EnvironmentConnectionState,
  blockedFailure: AttemptFailure | null
): string | null {
  if (state === "blocked" && blockedFailure !== null) {
    return BLOCKED_REASON[blockedFailure];
  }
  return ENVIRONMENT_STATUS_DOT[state].reason;
}
