import React from "react";

const TOOL_CALL_REGEX = /(\[(?:Read|Write|Edit|Bash|Glob|Grep|WebFetch|WebSearch)\s[^\]]*\])/g;
const TOOL_CALL_TEST = /^\[(?:Read|Write|Edit|Bash|Glob|Grep|WebFetch|WebSearch)\s/;

/** Extract tool call markers like [Read file.ts] from streaming text */
export function renderStreamContent(text: string): React.ReactNode[] {
  const parts = text.split(TOOL_CALL_REGEX);
  return parts.map((part, i) => {
    if (TOOL_CALL_TEST.test(part)) {
      return (
        <span
          key={i}
          className="inline-block text-[0.5625rem] px-1 py-px rounded mx-0.5"
          style={{
            backgroundColor: "var(--border-subtle)",
            color: "var(--accent-primary)",
          }}
        >
          {part}
        </span>
      );
    }
    return <span key={i}>{part}</span>;
  });
}
