#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
DB_PATH="$REPO_ROOT/src-tauri/ralphx.db"
RECENT_LIMIT=10

usage() {
  printf '%s\n' \
    'Usage:' \
    '  scripts/probe-memory-capture.sh [snapshot]' \
    '  scripts/probe-memory-capture.sh compare <before-entry-count> <before-event-count>' \
    '' \
    'Read-only A1 developer probe for the fixed src-tauri/ralphx.db.' \
    'Run snapshot before triggering memory capture, then pass those two counts to compare.'
}

fail() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

is_nonnegative_integer() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

command -v sqlite3 >/dev/null 2>&1 || fail 'sqlite3 is required'
[[ -f "$DB_PATH" ]] || fail 'fixed developer database src-tauri/ralphx.db was not found'

MODE="${1:-snapshot}"
case "$MODE" in
  snapshot)
    [[ $# -le 1 ]] || fail 'snapshot accepts no additional arguments'
    ;;
  compare)
    [[ $# -eq 3 ]] || fail 'compare requires before-entry-count and before-event-count'
    is_nonnegative_integer "$2" || fail 'before-entry-count must be a nonnegative integer'
    is_nonnegative_integer "$3" || fail 'before-event-count must be a nonnegative integer'
    ;;
  *)
    fail "unknown mode: $MODE"
    ;;
esac

ENTRY_COUNT="$(sqlite3 -readonly "$DB_PATH" 'SELECT COUNT(*) FROM memory_entries;')"
EVENT_COUNT="$(sqlite3 -readonly "$DB_PATH" 'SELECT COUNT(*) FROM memory_events;')"
is_nonnegative_integer "$ENTRY_COUNT" || fail 'memory entry count was not numeric'
is_nonnegative_integer "$EVENT_COUNT" || fail 'memory event count was not numeric'

printf 'database=%s\n' 'src-tauri/ralphx.db'
printf 'entries=%s\n' "$ENTRY_COUNT"
printf 'events=%s\n' "$EVENT_COUNT"

if [[ "$MODE" == "compare" ]]; then
  printf 'entry_delta=%s\n' "$((ENTRY_COUNT - $2))"
  printf 'event_delta=%s\n' "$((EVENT_COUNT - $3))"
fi

printf '\nRecent memory entries (limit %s)\n' "$RECENT_LIMIT"
sqlite3 -readonly -header -column "$DB_PATH" "
SELECT created_at,
       project_id,
       bucket,
       source_context_type,
       source_context_id,
       source_conversation_id,
       title
FROM memory_entries
ORDER BY created_at DESC
LIMIT $RECENT_LIMIT;
"

printf '\nRecent capture/skip events (limit %s)\n' "$RECENT_LIMIT"
sqlite3 -readonly -header -column "$DB_PATH" "
SELECT created_at,
       project_id,
       event_type,
       actor_type,
       json_extract(details_json, '$.reason') AS skip_reason,
       json_extract(details_json, '$.context_type') AS context_type,
       json_extract(details_json, '$.context_id') AS context_id,
       json_extract(details_json, '$.inserted') AS inserted,
       json_extract(details_json, '$.skipped') AS skipped,
       json_extract(details_json, '$.failed') AS failed
FROM memory_events
WHERE event_type IN (
  'memory_pipeline_spawn_requested',
  'memory_pipeline_skipped',
  'memory_capture_decision'
)
ORDER BY created_at DESC
LIMIT $RECENT_LIMIT;
"

printf '%s\n' \
  '' \
  'A1 checklist' \
  '- Completion requires a memory_entries delta, not only memory_pipeline_spawn_requested.' \
  '- A successful capture should include memory_capture_decision evidence.' \
  '- Project-scoped suppression should include one typed memory_pipeline_skipped reason.' \
  '- NoProjectId is trace-only and should not add a durable event.'
