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
