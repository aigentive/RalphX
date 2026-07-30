// GENERATED — do not edit; run node scripts/check-local-only-event-mirror.mjs --update

export const LOCAL_ONLY_BACKEND_EVENTS = [
  "gh-auth:login_prompt",
  "ralphx://check-for-updates",
  "ralphx://show-release-notes",
  "remote:device_paired",
  "remote:session_closed",
  "remote:session_connected",
  "remote:stream_closed",
  "remote:stream_frame",
] as const;

export const LOCAL_ONLY_BACKEND_EVENT_NAMES: ReadonlySet<string> = new Set(
  LOCAL_ONLY_BACKEND_EVENTS
);
