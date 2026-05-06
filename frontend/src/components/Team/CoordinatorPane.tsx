/**
 * CoordinatorPane — Lead/coordinator left pane
 *
 * Full-height flex column with:
 * - TeamOverviewHeader at top (compact stats bar)
 * - ChatMessageList in the middle (scrollable, filtered to lead messages)
 * - ChatInput at bottom for messaging the lead
 */

import React, { useMemo, useState, useCallback, useRef, useEffect } from "react";
import { useTeamStore, selectActiveTeam, selectTeamMessages } from "@/stores/teamStore";
import { TeamOverviewHeader } from "./TeamOverviewHeader";
import type { TeamMessage } from "@/stores/teamStore";

interface CoordinatorPaneProps {
  contextKey: string;
  onSendMessage?: (content: string) => void;
}

function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}

export const CoordinatorPane = React.memo(function CoordinatorPane({
  contextKey,
  onSendMessage,
}: CoordinatorPaneProps) {
  const teamSelector = useMemo(() => selectActiveTeam(contextKey), [contextKey]);
  const team = useTeamStore(teamSelector);
  const messagesSelector = useMemo(() => selectTeamMessages(contextKey), [contextKey]);
  const messages = useTeamStore(messagesSelector);

  const [inputValue, setInputValue] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  // Filter to lead messages (from or to the lead)
  const leadMessages = useMemo(() => {
    if (!team) return [];
    return messages.filter(
      (msg: TeamMessage) => msg.from === team.leadName || msg.to === team.leadName,
    );
  }, [messages, team]);

  // Auto-scroll on new messages
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [leadMessages.length]);

  const handleSend = useCallback(() => {
    const trimmed = inputValue.trim();
    if (!trimmed || !onSendMessage) return;
    onSendMessage(trimmed);
    setInputValue("");
  }, [inputValue, onSendMessage]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      }
    },
    [handleSend],
  );

  if (!team) return null;

  return (
    <div className="flex flex-col h-full" style={{ backgroundColor: "var(--bg-base)" }}>
      {/* Stats header */}
      <TeamOverviewHeader contextKey={contextKey} />

      {/* Message list */}
      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto px-3 py-2 space-y-2"
      >
        {leadMessages.length === 0 ? (
          <p className="text-[0.6875rem] text-center py-8" style={{ color: "var(--text-muted)" }}>
            No messages yet
          </p>
        ) : (
          leadMessages.map((msg: TeamMessage) => {
            const isFromLead = msg.from === team.leadName;
            return (
              <div key={msg.id} className="flex flex-col gap-0.5">
                <div className="flex items-baseline gap-1.5">
                  <span
                    className="text-[0.6875rem] font-medium"
                    style={{ color: isFromLead ? "var(--accent-primary)" : "var(--text-secondary)" }}
                  >
                    {msg.from}
                  </span>
                  {msg.to && (
                    <>
                      <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>→</span>
                      <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>{msg.to}</span>
                    </>
                  )}
                  <span className="text-[0.5625rem] ml-auto" style={{ color: "var(--text-muted)" }}>
                    {formatTimestamp(msg.timestamp)}
                  </span>
                </div>
                <p
                  className="text-[0.75rem] leading-relaxed"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {msg.content}
                </p>
              </div>
            );
          })
        )}
      </div>

      {/* Input */}
      <div
        className="flex items-center gap-2 px-3 shrink-0"
        style={{
          height: 36,
          borderTop: "1px solid var(--border-subtle)",
        }}
      >
        <input
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={`Message ${team.leadName}...`}
          className="flex-1 text-[0.75rem] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
          style={{
            color: "var(--text-primary)",
            boxShadow: "none",
            outline: "none",
          }}
        />
        <button
          type="button"
          onClick={handleSend}
          disabled={!inputValue.trim() || !onSendMessage}
          className="text-[0.6875rem] px-2 py-0.5 rounded disabled:opacity-30 transition-opacity"
          style={{
            backgroundColor: "var(--accent-primary)",
            color: "white",
          }}
        >
          Send
        </button>
      </div>
    </div>
  );
});
