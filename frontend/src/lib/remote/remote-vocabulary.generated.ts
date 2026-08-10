// GENERATED — do not edit; run node scripts/check-remote-vocabulary-mirror.mjs --update
//
// Source: src-tauri/crates/ralphx-remote-protocol/tests/snapshots/vocabulary.json

export const PROTOCOL_TRANSPORT_ERROR_CODES = [
  "REMOTE_COMMAND_UNAVAILABLE",
  "REMOTE_FORBIDDEN",
  "REMOTE_UNAUTHORIZED",
  "REMOTE_UNREACHABLE",
  "REMOTE_VERSION_MISMATCH",
  "REMOTE_TIMEOUT_UNKNOWN",
  "REMOTE_REQUEST_IN_PROGRESS",
  "REMOTE_REQUEST_ID_REUSED",
  "REMOTE_INVALID_ARGUMENTS",
  "REMOTE_INTERNAL_ERROR",
] as const;

export const PROTOCOL_RESET_REASONS = [
  "cursor_pruned",
  "epoch_changed",
  "after_seq_gt_max",
  "read_error",
  "revoked",
  "host_disabled",
] as const;
