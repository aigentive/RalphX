/**
 * TeamSystemEvent — Team lifecycle event display
 *
 * Centered, muted style for events like "coder-3 joined the team", "Wave 2 validated".
 */

import React from "react";

interface TeamSystemEventProps {
  message: string;
  timestamp?: string | undefined;
}

export const TeamSystemEvent = React.memo(function TeamSystemEvent({
  message,
  timestamp,
}: TeamSystemEventProps) {
  return (
    <div className="flex items-center justify-center gap-2 py-2">
      <div
        className="text-[0.6875rem] px-3 py-1 rounded-full"
        style={{
          color: "var(--text-muted)",
          backgroundColor: "var(--bg-surface)",
        }}
      >
        {message}
        {timestamp && (
          <span className="ml-2 opacity-50">
            {new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </span>
        )}
      </div>
    </div>
  );
});
