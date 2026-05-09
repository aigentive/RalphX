/**
 * QueuedMessage - Component for displaying a queued message
 *
 * Displays a message that will be sent when the agent finishes.
 * Features:
 * - Edit mode (inline editing)
 * - Delete action
 * - Pending/queued visual style (muted, send icon)
 */

import { useState, useCallback, useEffect } from "react";
import { Check, Pencil, X } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatQueuedMessageExcerpt } from "@/lib/queuedMessageExcerpt";
import type { QueuedMessage as QueuedMessageType } from "@/stores/chatStore";

// ============================================================================
// Icons
// ============================================================================

function SendIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
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

// ============================================================================
// Types
// ============================================================================

export interface QueuedMessageProps {
  /** The queued message to display */
  message: QueuedMessageType;
  /** Callback when edit is confirmed */
  onEdit: (id: string, content: string) => void;
  /** Callback when delete is requested */
  onDelete: (id: string) => void;
}

// ============================================================================
// Component
// ============================================================================

export function QueuedMessage({ message, onEdit, onDelete }: QueuedMessageProps) {
  const [isEditing, setIsEditing] = useState(message.isEditing);
  const [editContent, setEditContent] = useState(message.content);
  const previewContent = formatQueuedMessageExcerpt(message.content);

  useEffect(() => {
    setIsEditing(message.isEditing);
    if (message.isEditing) {
      setEditContent(message.content);
    }
  }, [message.content, message.isEditing]);

  const handleStartEdit = useCallback(() => {
    setIsEditing(true);
    setEditContent(message.content);
  }, [message.content]);

  const handleSaveEdit = useCallback(() => {
    if (editContent.trim()) {
      onEdit(message.id, editContent.trim());
      setIsEditing(false);
    }
  }, [message.id, editContent, onEdit]);

  const handleCancelEdit = useCallback(() => {
    setIsEditing(false);
    setEditContent(message.content);
  }, [message.content]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSaveEdit();
      } else if (e.key === "Escape") {
        e.preventDefault();
        handleCancelEdit();
      }
    },
    [handleSaveEdit, handleCancelEdit]
  );

  const handleDelete = useCallback(() => {
    onDelete(message.id);
  }, [message.id, onDelete]);

  return (
    <div
      data-testid="queued-message"
      data-message-id={message.id}
      className="rounded-lg p-3 transition-all"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <div className="flex items-start gap-2">
        {/* Send icon indicator */}
        <div className="flex-shrink-0 mt-1" style={{ color: "var(--text-muted)" }}>
          <SendIcon />
        </div>

        {/* Content area */}
        <div className="flex-1 min-w-0">
          {isEditing ? (
            <textarea
              data-testid="queued-message-edit-input"
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
              onKeyDown={handleKeyDown}
              autoFocus
              className="w-full px-2 py-1 text-sm rounded resize-none outline-none focus:ring-1 focus:ring-offset-0"
              style={{
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-primary)",
                minHeight: "40px",
              }}
              rows={2}
            />
          ) : (
            <p
              data-testid="queued-message-content"
              className="text-sm break-words"
              style={{ color: "var(--text-secondary)" }}
            >
              {previewContent}
            </p>
          )}
        </div>

        {/* Actions */}
        <TooltipProvider>
          <div className="flex items-start gap-1 flex-shrink-0">
            {isEditing ? (
              <>
                {/* Save button */}
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      data-testid="queued-message-save"
                      onClick={handleSaveEdit}
                      disabled={!editContent.trim()}
                      className="p-1 rounded transition-colors hover:bg-opacity-80 disabled:opacity-30"
                      style={{ color: "var(--status-success)" }}
                      aria-label="Save edit"
                    >
                      <Check size={16} />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">Save edit</TooltipContent>
                </Tooltip>
                {/* Cancel button */}
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      data-testid="queued-message-cancel"
                      onClick={handleCancelEdit}
                      className="p-1 rounded transition-colors hover:bg-opacity-80"
                      style={{ color: "var(--text-muted)" }}
                      aria-label="Cancel edit"
                    >
                      <X size={16} />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">Cancel edit</TooltipContent>
                </Tooltip>
              </>
            ) : (
              <>
                {/* Edit button */}
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      data-testid="queued-message-edit"
                      onClick={handleStartEdit}
                      className="p-1 rounded transition-colors hover:bg-opacity-80"
                      style={{ color: "var(--text-muted)" }}
                      aria-label="Edit message"
                    >
                      <Pencil size={16} />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">Edit message</TooltipContent>
                </Tooltip>
                {/* Delete button */}
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      data-testid="queued-message-delete"
                      onClick={handleDelete}
                      className="p-1 rounded transition-colors hover:bg-opacity-80"
                      style={{ color: "var(--status-error)" }}
                      aria-label="Delete message"
                    >
                      <X size={16} />
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top">Delete message</TooltipContent>
                </Tooltip>
              </>
            )}
          </div>
        </TooltipProvider>
      </div>
    </div>
  );
}
