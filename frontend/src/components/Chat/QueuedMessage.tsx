/**
 * QueuedMessage - Component for displaying a queued message
 *
 * Displays a message that will be sent when the agent finishes.
 * Features:
 * - Edit mode (inline editing)
 * - Delete action
 * - Pending/queued visual style (muted, send icon)
 */

import { useState, useCallback, useEffect, useMemo } from "react";
import { Check, Paperclip, Pencil, SendHorizontal, X } from "lucide-react";
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
  onEdit: (
    id: string,
    content: string,
    attachmentIds?: string[],
    selectionSnapshot?: QueuedMessageType["composerSelectionSnapshot"],
  ) => void;
  /** Callback when delete is requested */
  onDelete: (id: string) => void;
  /** Callback when this queued message should interrupt the active run and send now */
  onSendNow?: (
    id: string,
    content: string,
    attachmentIds?: string[],
    selectionSnapshot?: QueuedMessageType["composerSelectionSnapshot"],
  ) => void;
}

// ============================================================================
// Component
// ============================================================================

export function QueuedMessage({ message, onEdit, onDelete, onSendNow }: QueuedMessageProps) {
  const [isEditing, setIsEditing] = useState(message.isEditing);
  const [editContent, setEditContent] = useState(message.content);
  const previewContent = formatQueuedMessageExcerpt(message.content);
  const attachmentIds = useMemo(() => message.attachmentIds ?? [], [message.attachmentIds]);
  const attachmentCount = attachmentIds.length;

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
      if (message.composerSelectionSnapshot) {
        onEdit(
          message.id,
          editContent.trim(),
          attachmentIds,
          message.composerSelectionSnapshot,
        );
      } else {
        onEdit(message.id, editContent.trim(), attachmentIds);
      }
      setIsEditing(false);
    }
  }, [message.id, message.composerSelectionSnapshot, attachmentIds, editContent, onEdit]);

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

  const handleSendNow = useCallback(() => {
    if (message.composerSelectionSnapshot) {
      onSendNow?.(
        message.id,
        message.content,
        attachmentIds,
        message.composerSelectionSnapshot,
      );
    } else {
      onSendNow?.(message.id, message.content, attachmentIds);
    }
  }, [
    message.content,
    message.id,
    message.composerSelectionSnapshot,
    attachmentIds,
    onSendNow,
  ]);

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
            <>
              <p
                data-testid="queued-message-content"
                className="text-sm break-words"
                style={{ color: "var(--text-secondary)" }}
              >
                {previewContent}
              </p>
              {attachmentCount > 0 && (
                <div
                  data-testid="queued-message-attachment-count"
                  className="mt-1 inline-flex items-center gap-1 text-xs"
                  style={{ color: "var(--text-muted)" }}
                  aria-label={`${attachmentCount} queued attachment${attachmentCount === 1 ? "" : "s"}`}
                >
                  <Paperclip size={12} aria-hidden="true" />
                  <span>
                    {attachmentCount} attachment{attachmentCount === 1 ? "" : "s"}
                  </span>
                </div>
              )}
            </>
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
                {onSendNow && (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        data-testid="queued-message-send-now"
                        onClick={handleSendNow}
                        className="p-1 rounded transition-colors hover:bg-opacity-80"
                        style={{ color: "var(--accent-primary)" }}
                        aria-label="Send queued message now"
                      >
                        <SendHorizontal size={16} />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="top">Send now</TooltipContent>
                  </Tooltip>
                )}
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
