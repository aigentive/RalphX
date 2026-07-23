#!/bin/bash

# find-debug-logs.sh — Search Claude Code debug logs by date, agent name, or keyword
# Usage: find-debug-logs.sh [options]
#
# Options:
#   -d, --date DATE          Search for logs from specific date (YYYY-MM-DD)
#   -a, --agent NAME         Search for agent name (partial match)
#   -k, --keywords WORDS     Search for keywords (comma-separated, any match)
#   -t, --time HH:MM         Filter by file birth time (local, prefix match)
#   -v, --verbose            Show context + sample matches
#   -h, --help               Show this help message
#
# Examples:
#   find-debug-logs.sh -d 2026-02-24 -t 12:13        # Files born on date near time
#   find-debug-logs.sh -a "frontend-researcher" -v    # Content grep for agent name

DEBUG_DIR="$HOME/.claude/debug"
DATE=""
AGENT_NAME=""
KEYWORDS=""
TIME_FILTER=""
VERBOSE=0

show_help() {
  sed -n '3,25p' "$0" | sed 's/^# //'
}

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    -d|--date) DATE="$2"; shift 2 ;;
    -a|--agent) AGENT_NAME="$2"; shift 2 ;;
    -k|--keywords) KEYWORDS="$2"; shift 2 ;;
    -t|--time) TIME_FILTER="$2"; shift 2 ;;
    -v|--verbose) VERBOSE=1; shift ;;
    -h|--help) show_help; exit 0 ;;
    *) echo "Error: Unknown option $1"; show_help; exit 1 ;;
  esac
done

# ── Content search mode (original) ──────────────────────────────────────────

# Validate at least one criterion
if [ -z "$DATE" ] && [ -z "$AGENT_NAME" ] && [ -z "$KEYWORDS" ] && [ -z "$TIME_FILTER" ]; then
  echo "Error: Specify at least one search criterion (-d, -a, -k, or -t)"
  show_help
  exit 1
fi

# If only date + time filter (no content search), use birth-time mode
if [ -n "$DATE" ] && [ -z "$AGENT_NAME" ] && [ -z "$KEYWORDS" ]; then
  # Convert YYYY-MM-DD to month abbreviation + day for stat output matching
  _MONTH_NUM=$(echo "$DATE" | cut -d'-' -f2)
  _DAY_NUM=$(echo "$DATE" | cut -d'-' -f3 | sed 's/^0/ /')
  case "$_MONTH_NUM" in
    01) _MONTH_ABR="Jan";; 02) _MONTH_ABR="Feb";; 03) _MONTH_ABR="Mar";;
    04) _MONTH_ABR="Apr";; 05) _MONTH_ABR="May";; 06) _MONTH_ABR="Jun";;
    07) _MONTH_ABR="Jul";; 08) _MONTH_ABR="Aug";; 09) _MONTH_ABR="Sep";;
    10) _MONTH_ABR="Oct";; 11) _MONTH_ABR="Nov";; 12) _MONTH_ABR="Dec";;
  esac
  _DATE_PATTERN="${_MONTH_ABR} ${_DAY_NUM}"

  echo "Searching debug logs by file birth time..."
  echo "   Date: $DATE ($_DATE_PATTERN)"
  [ -n "$TIME_FILTER" ] && echo "   Time: $TIME_FILTER"
  echo ""

  stat -f "%SB %N" "$DEBUG_DIR"/*.txt 2>/dev/null | grep "$_DATE_PATTERN" | while IFS= read -r line; do
    # Filter by time if specified
    if [ -n "$TIME_FILTER" ] && ! echo "$line" | grep -q "$TIME_FILTER" 2>/dev/null; then
      continue
    fi
    filepath=$(echo "$line" | awk '{print $NF}')
    filename=$(basename "$filepath")
    birthtime=$(echo "$line" | sed "s| $filepath||")
    size=$(ls -lh "$filepath" 2>/dev/null | awk '{print $5}')
    lines=$(wc -l < "$filepath" 2>/dev/null | xargs)
    printf "  %-40s  %7s  %5s lines  Born: %s\n" "$filename" "$size" "$lines" "$birthtime"
  done | sort

  exit 0
fi

# Build grep patterns
declare -a PATTERNS

[ -n "$AGENT_NAME" ] && PATTERNS+=("$AGENT_NAME")

if [ -n "$KEYWORDS" ]; then
  IFS=',' read -ra KEYWORD_ARRAY <<< "$KEYWORDS"
  for kw in "${KEYWORD_ARRAY[@]}"; do
    PATTERNS+=("$(echo "$kw" | xargs)")
  done
fi

# Fallback: if only date given with keywords/agent, add date to patterns
[ -n "$DATE" ] && [ ${#PATTERNS[@]} -eq 0 ] && PATTERNS+=("$DATE")

# Create regex pattern
if [ ${#PATTERNS[@]} -eq 1 ]; then
  PATTERN="${PATTERNS[0]}"
  GREP_OPTS="-i"
else
  # OR pattern for multiple criteria
  PATTERN=$(printf '%s\|' "${PATTERNS[@]}" | sed 's/\\|$//')
  GREP_OPTS="-iE"
fi

# Pre-filter files by birth date if -d provided
declare -a FILE_LIST
if [ -n "$DATE" ]; then
  # Map date to month abbreviation for stat output matching
  MONTH_NUM=$(echo "$DATE" | cut -d'-' -f2)
  DAY_NUM=$(echo "$DATE" | cut -d'-' -f3 | sed 's/^0//')
  case "$MONTH_NUM" in
    01) MONTH_ABR="Jan";; 02) MONTH_ABR="Feb";; 03) MONTH_ABR="Mar";;
    04) MONTH_ABR="Apr";; 05) MONTH_ABR="May";; 06) MONTH_ABR="Jun";;
    07) MONTH_ABR="Jul";; 08) MONTH_ABR="Aug";; 09) MONTH_ABR="Sep";;
    10) MONTH_ABR="Oct";; 11) MONTH_ABR="Nov";; 12) MONTH_ABR="Dec";;
  esac

  while IFS= read -r line; do
    filepath=$(echo "$line" | awk '{print $NF}')
    if [ -n "$TIME_FILTER" ] && ! echo "$line" | grep -q "$TIME_FILTER"; then
      continue
    fi
    FILE_LIST+=("$filepath")
  done < <(stat -f "%SB %N" "$DEBUG_DIR"/*.txt 2>/dev/null | grep "$MONTH_ABR" | grep " $DAY_NUM ")
else
  for f in "$DEBUG_DIR"/*.txt; do
    [ -f "$f" ] && FILE_LIST+=("$f")
  done
fi

# Search and display results
echo "Searching debug logs..."
[ -n "$DATE" ] && echo "   Date: $DATE"
[ -n "$TIME_FILTER" ] && echo "   Time: $TIME_FILTER"
[ -n "$AGENT_NAME" ] && echo "   Agent: $AGENT_NAME"
[ -n "$KEYWORDS" ] && echo "   Keywords: $KEYWORDS"
echo ""

MATCHES=()
for file in "${FILE_LIST[@]}"; do
  if [ -f "$file" ]; then
    count=$(grep $GREP_OPTS "$PATTERN" "$file" 2>/dev/null | wc -l | xargs)
    if [ "$count" -gt 0 ]; then
      filename=$(basename "$file")
      size=$(ls -lh "$file" | awk '{print $5}')
      birthtime=$(stat -f "%SB" "$file" 2>/dev/null)
      MATCHES+=("$filename:$size:$birthtime:$count")
      printf "  %-40s  %7s  Born: %-24s  (%d matches)\n" "$filename" "$size" "$birthtime" "$count"
    fi
  fi
done

if [ ${#MATCHES[@]} -eq 0 ]; then
  echo "No matching debug logs found"
  exit 1
fi

echo ""
echo "Found ${#MATCHES[@]} matching file(s)"

# Show sample if verbose
if [ "$VERBOSE" -eq 1 ] && [ ${#MATCHES[@]} -gt 0 ]; then
  IFS=':' read -r first_file rest <<< "${MATCHES[0]}"
  echo ""
  echo "Sample from: $first_file (first 5 matches)"
  echo "  ---"
  grep $GREP_OPTS "$PATTERN" "$DEBUG_DIR/$first_file" 2>/dev/null | head -5 | sed 's/^/  /'
  echo ""
fi

echo ""
echo "Tip: Use -v/--verbose to see sample lines"
echo "     Use -s 'session title' for DB cross-reference mode"
