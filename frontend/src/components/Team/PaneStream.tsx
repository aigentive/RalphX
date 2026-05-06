/**
 * PaneStream — Streaming text display for a teammate pane
 *
 * Previously showed raw streamingText from teamStore.
 * Now teammate messages are loaded via useConversation in the chat panel.
 * This component shows a placeholder directing users to the teammate tab.
 */

import React from "react";

interface PaneStreamProps {
  contextKey: string;
  teammateName: string;
}

export const PaneStream = React.memo(function PaneStream({
  teammateName,
}: PaneStreamProps) {
  return (
    <div
      className="flex-1 overflow-y-auto px-2.5 py-2 flex items-center justify-center"
      style={{ backgroundColor: "var(--bg-base)" }}
    >
      <p
        className="text-[0.6875rem] text-center py-4"
        style={{ color: "var(--text-muted)" }}
      >
        View {teammateName}&apos;s output in the teammate tab
      </p>
    </div>
  );
});
