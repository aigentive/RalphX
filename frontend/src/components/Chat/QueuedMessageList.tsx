/**
 * QueuedMessageList - Component for displaying queued messages
 *
 * Shows all messages queued to be sent when the agent finishes.
 * Features:
 * - Header explaining queue behavior
 * - List of QueuedMessage components
 * - Only shows if queue is not empty
 */

import { QueuedMessage } from "./QueuedMessage";
import type { QueuedMessage as QueuedMessageType } from "@/stores/chatStore";

// ============================================================================
// Types
// ============================================================================

export interface QueuedMessageListProps {
  /** Array of queued messages */
  messages: QueuedMessageType[];
  /** Callback when a message is edited */
  onEdit: (id: string, content: string) => void;
  /** Callback when a message is deleted */
  onDelete: (id: string) => void;
}

// ============================================================================
// Component
// ============================================================================

export function QueuedMessageList({ messages, onEdit, onDelete }: QueuedMessageListProps) {
  // Don't render if queue is empty
  if (messages.length === 0) {
    return null;
  }

  return (
    <div
      data-testid="queued-message-list"
      className="rounded-lg p-4 mb-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {/* Header */}
      <div className="flex items-center gap-2 mb-3">
        <h3
          className="text-xs font-medium uppercase tracking-wide"
          style={{ color: "var(--text-muted)" }}
        >
          Queued Messages ({messages.length})
        </h3>
        <div
          className="flex-1 h-px"
          style={{ backgroundColor: "var(--border-subtle)" }}
        />
      </div>

      <p
        className="text-xs mb-3"
        style={{ color: "var(--text-secondary)" }}
      >
        These messages will be sent when the agent finishes.
      </p>

      {/* Messages */}
      <div className="flex flex-col gap-2">
        {messages.map((message) => (
          <QueuedMessage
            key={message.id}
            message={message}
            onEdit={onEdit}
            onDelete={onDelete}
          />
        ))}
      </div>
    </div>
  );
}
