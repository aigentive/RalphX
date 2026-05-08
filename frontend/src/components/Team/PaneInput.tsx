/**
 * PaneInput — Compact chat input for a teammate pane
 *
 * Single-line text input with send button.
 * Disabled when teammate status is "shutdown" or "completed".
 * Height: 36px with border-top separator.
 */

import React, { useState, useCallback } from "react";
import type { TeammateStatus } from "@/stores/teamStore";

interface PaneInputProps {
  teammateName: string;
  status: TeammateStatus;
  onSend: (content: string) => void;
}

function SendIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path
        d="M14 2L2 7.5L6.5 9.5M14 2L9.5 14L6.5 9.5M14 2L6.5 9.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export const PaneInput = React.memo(function PaneInput({
  teammateName,
  status,
  onSend,
}: PaneInputProps) {
  const [value, setValue] = useState("");
  const isDisabled = status === "shutdown" || status === "completed";

  const handleSend = useCallback(() => {
    const trimmed = value.trim();
    if (!trimmed || isDisabled) return;
    onSend(trimmed);
    setValue("");
  }, [value, isDisabled, onSend]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  return (
    <div
      className="flex items-center gap-1.5 px-2.5 shrink-0"
      style={{
        height: 36,
        borderTop: "1px solid var(--border-subtle)",
      }}
    >
      <input
        type="text"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={`Message ${teammateName}...`}
        disabled={isDisabled}
        className="flex-1 text-[0.6875rem] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0 disabled:opacity-40"
        style={{
          color: "var(--text-primary)",
          boxShadow: "none",
          outline: "none",
        }}
      />
      <button
        type="button"
        onClick={handleSend}
        disabled={!value.trim() || isDisabled}
        aria-label={`Send message to ${teammateName}`}
        className="w-6 h-6 flex items-center justify-center rounded disabled:opacity-20 transition-opacity"
        style={{
          color: "var(--accent-primary)",
        }}
      >
        <SendIcon />
      </button>
    </div>
  );
});
