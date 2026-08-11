/**
 * ChatInput - Reusable chat input component
 *
 * Design spec: specs/design/pages/chat-panel.md
 * - Refined Studio aesthetic with layered depth
 * - Gradient background on textarea
 * - Orange accent send button
 * - Compact sizing for application UI
 */

import {
  useState,
  useRef,
  useCallback,
  useEffect,
  type ReactNode,
} from "react";
import { useChatAttachmentDrop } from "@/hooks/useChatAttachmentDrop";
import { ChatAttachmentPicker } from "./ChatAttachmentPicker";
import { ChatAttachmentDropOverlay } from "./ChatAttachmentDropOverlay";
import { ChatAttachmentGallery, type ChatAttachment } from "./ChatAttachmentGallery";
import type { AgentStatus } from "@/stores/chatStore";

// ============================================================================
// Types
// ============================================================================

export interface QuestionMode {
  /** Number of available options (for placeholder "Type 1-N") */
  optionCount: number;
  /** Whether multiple options can be selected */
  multiSelect: boolean;
  /** Called with matched option indices (0-based) when input matches number patterns */
  onMatchedOptions: (indices: number[]) => void;
}

export interface ChatInputProps {
  /** Callback when message is sent */
  onSend: (message: string) => Promise<void> | void;
  /** Placeholder text for the textarea */
  placeholder?: string;
  /** Whether a message is currently being sent */
  isSending?: boolean;
  /** Controlled value for the textarea */
  value?: string;
  /** Callback when value changes (for controlled mode) */
  onChange?: (value: string) => void;
  /** Show helper text about keyboard shortcuts */
  showHelperText?: boolean;
  /** Auto-focus the textarea on mount */
  autoFocus?: boolean;
  /** Whether an agent is currently running (enables queue mode). Deprecated: prefer agentStatus. */
  isAgentRunning?: boolean;
  /** Tri-state agent status — overrides isAgentRunning when provided */
  agentStatus?: AgentStatus;
  /** Whether there are queued messages */
  hasQueuedMessages?: boolean;
  /** Callback to edit the last queued message */
  onEditLastQueued?: () => void;
  /** Callback to stop the running agent */
  onStop?: () => void;
  /** Whether the input is in read-only mode (e.g., viewing historical state) */
  isReadOnly?: boolean;
  /** Question-aware mode: changes placeholder, enables number matching, updates helper text */
  questionMode?: QuestionMode;
  /** Enable file attachment picker */
  enableAttachments?: boolean;
  /** Array of file attachments */
  attachments?: ChatAttachment[];
  /** Callback when files are selected */
  onFilesSelected?: (files: File[]) => void;
  /** Callback when attachment is removed */
  onRemoveAttachment?: (id: string) => void;
  /** Optional conversation persona confirmation rendered in the footer */
  personaControl?: ReactNode;
}

// ============================================================================
// Icons
// ============================================================================

function SendIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
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

function LoadingSpinner() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      className="animate-spin"
    >
      <circle
        cx="8"
        cy="8"
        r="6"
        stroke="currentColor"
        strokeWidth="2"
        strokeOpacity="0.3"
      />
      <path
        d="M14 8A6 6 0 0 0 8 2"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ============================================================================
// Component
// ============================================================================

export function ChatInput({
  onSend,
  placeholder = "Send a message...",
  isSending = false,
  value: controlledValue,
  onChange: onChangeProp,
  showHelperText = true,
  autoFocus = false,
  isAgentRunning = false,
  agentStatus: agentStatusProp,
  hasQueuedMessages = false,
  onEditLastQueued,
  onStop,
  isReadOnly = false,
  questionMode,
  enableAttachments = false,
  attachments,
  onFilesSelected,
  onRemoveAttachment,
  personaControl,
}: ChatInputProps) {
  // Derive agent state from tri-state when available, fall back to boolean
  const effectiveStatus: AgentStatus = agentStatusProp ?? (isAgentRunning ? "generating" : "idle");
  const isAgentAlive = effectiveStatus !== "idle";
  const isAgentGenerating = effectiveStatus === "generating";
  // Support both controlled and uncontrolled modes
  const [internalValue, setInternalValue] = useState("");
  const isControlled = controlledValue !== undefined;
  const value = isControlled ? controlledValue : internalValue;

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const composerRef = useRef<HTMLDivElement>(null);

  // Determine the actual placeholder text
  const effectivePlaceholder = isReadOnly
    ? "Viewing historical state (read-only)"
    : questionMode
      ? `Type 1-${questionMode.optionCount} or a custom response...`
      : placeholder;

  // Auto-focus on mount if requested
  useEffect(() => {
    if (autoFocus && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [autoFocus]);

  // Auto-resize textarea based on content
  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;

    // Reset height to auto to get the correct scrollHeight
    textarea.style.height = "auto";
    // Set height to scrollHeight, capped at maxHeight (120px)
    const newHeight = Math.min(textarea.scrollHeight, 120);
    textarea.style.height = `${newHeight}px`;
  }, [value]);

  // Parse input for number matching in question mode
  const matchOptionsFromInput = useCallback(
    (input: string) => {
      if (!questionMode) return;
      const trimmed = input.trim();
      if (!trimmed) {
        questionMode.onMatchedOptions([]);
        return;
      }

      if (questionMode.multiSelect) {
        // Multi-select: parse comma-separated numbers like "1,3" or "1, 3, 5"
        const parts = trimmed.split(",").map((s) => s.trim());
        const allNumeric = parts.every((p) => /^\d+$/.test(p));
        if (allNumeric) {
          const indices = parts
            .map((p) => parseInt(p, 10))
            .filter((n) => n >= 1 && n <= questionMode.optionCount)
            .map((n) => n - 1); // Convert 1-based to 0-based
          questionMode.onMatchedOptions(indices);
        } else {
          questionMode.onMatchedOptions([]);
        }
      } else {
        // Single-select: match a single number like "1", "2", "3"
        if (/^\d+$/.test(trimmed)) {
          const num = parseInt(trimmed, 10);
          if (num >= 1 && num <= questionMode.optionCount) {
            questionMode.onMatchedOptions([num - 1]);
          } else {
            questionMode.onMatchedOptions([]);
          }
        } else {
          questionMode.onMatchedOptions([]);
        }
      }
    },
    [questionMode]
  );

  // Handle value changes
  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const newValue = e.target.value;
      if (isControlled) {
        onChangeProp?.(newValue);
      } else {
        setInternalValue(newValue);
      }
      matchOptionsFromInput(newValue);
    },
    [isControlled, onChangeProp, matchOptionsFromInput]
  );

  // Handle sending or queueing message
  const handleSend = useCallback(async () => {
    const trimmedValue = value.trim();
    // Block if no content, or if sending and agent not alive (can't queue or interact)
    if (!trimmedValue || (isSending && !isAgentAlive)) return;

    // Clear input immediately (optimistic UI)
    const clearInput = () => {
      if (isControlled) {
        onChangeProp?.("");
      } else {
        setInternalValue("");
      }
      // Clear any matched options when input is cleared
      questionMode?.onMatchedOptions([]);
    };

    if (questionMode) {
      // Question answers must be delivered immediately — never queue
      // Don't clearInput() here — let handleQuestionSend clear after successful submission.
      // Premature clearing causes lost input when the backend call fails (e.g., stale session).
      await onSend(trimmedValue);
    } else {
      clearInput();
      try {
        await onSend(trimmedValue);
      } catch {
        // Message was already cleared - error will be shown elsewhere
      }
    }
  }, [
    value,
    isSending,
    isAgentAlive,
    onSend,
    isControlled,
    onChangeProp,
    questionMode,
  ]);

  // Handle keyboard shortcuts
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Escape") {
        e.preventDefault();
        (e.target as HTMLTextAreaElement).blur();
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSend();
      } else if (e.key === "ArrowUp" && !value && hasQueuedMessages) {
        // Up arrow in empty input: edit last queued message
        e.preventDefault();
        onEditLastQueued?.();
      }
    },
    [handleSend, value, hasQueuedMessages, onEditLastQueued]
  );

  // Track focus state for unified container border highlight
  const [isFocused, setIsFocused] = useState(false);

  // Allow typing and queueing/sending when agent is alive (generating or waiting), but not in read-only mode
  const isDisabled = isReadOnly || (isSending && !isAgentAlive);
  const canSend = value.trim().length > 0 && !isReadOnly && (!isSending || isAgentAlive);
  const attachmentDropEnabled = enableAttachments && !isReadOnly && onFilesSelected !== undefined;
  const { isDragging: isAttachmentDragging, dropProps: attachmentDropProps } = useChatAttachmentDrop({
    enabled: attachmentDropEnabled,
    targetRef: composerRef,
    onFilesSelected,
  });

  return (
    <div
      ref={composerRef}
      data-testid="chat-input"
      className="relative flex flex-col"
      {...attachmentDropProps}
    >
      <div className="flex gap-2 items-end">
        {/* Unified container: textarea + stop button share one input field.
            Border is 2px wide on every state so the focus color swap does
            not reflow the layout (previously 1px→2px toggled a 1px jump). */}
        <div
          data-testid="chat-input-field"
          className="chat-input-field flex-1 flex items-end rounded-lg transition-colors"
          style={{
            background: "var(--bg-surface)",
            border: isFocused
              ? "2px solid var(--border-default)"
              : "2px solid var(--bg-hover)",
            minHeight: "38px",
          }}
        >
          {/* Textarea - transparent inside the unified container */}
          <textarea
            ref={textareaRef}
            data-testid="chat-input-textarea"
            value={value}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            onFocus={() => setIsFocused(true)}
            onBlur={() => setIsFocused(false)}
            disabled={isDisabled}
            placeholder={effectivePlaceholder}
            rows={1}
            aria-label="Message input"
            className="flex-1 px-3 py-2 text-[0.8125rem] resize-none outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0 focus:border-0"
            style={{
              background: "transparent",
              color: "var(--text-primary)",
              border: "none",
              minHeight: "34px",
              maxHeight: "120px",
              overflowY: "auto",
              boxShadow: "none",
              outline: "none",
            }}
          />

          {/* Stop icon — inside container, right side, subtle icon only */}
          {isAgentGenerating && onStop && !isReadOnly && (
            <div className="pr-1 pb-1 flex-shrink-0">
              <button
                data-testid="chat-input-stop"
                type="button"
                onClick={onStop}
                aria-label="Stop agent"
                className="p-1.5 rounded transition-colors"
                style={{ color: "var(--text-muted)" }}
                onMouseEnter={(e: React.MouseEvent<HTMLButtonElement>) => {
                  e.currentTarget.style.color = "var(--accent-primary)";
                }}
                onMouseLeave={(e: React.MouseEvent<HTMLButtonElement>) => {
                  e.currentTarget.style.color = "var(--text-muted)";
                }}
              >
                <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                  <rect x="3" y="3" width="10" height="10" rx="1" />
                </svg>
              </button>
            </div>
          )}
        </div>

        {/* Attachment picker — sits BETWEEN the input box and the Send
            button per 2026-04-19 feedback (Image #55). Uses subtle=false
            so the paperclip button renders at 38×38 matching the Send
            button's height and rounded-lg chrome. */}
        {enableAttachments && (
          <div className="flex-shrink-0 flex items-center">
            <ChatAttachmentPicker
              {...(onFilesSelected !== undefined && { onFilesSelected })}
              disabled={isReadOnly}
              subtle={false}
            />
          </div>
        )}

        {/* Send button */}
        <div className="flex items-center">
          <button
            data-testid="chat-input-send"
            type="button"
            onClick={handleSend}
            disabled={!canSend}
            aria-label="Send message"
            aria-busy={isSending}
            className="px-3 py-2 rounded-lg transition-colors disabled:opacity-[0.54] shrink-0 h-[38px] flex items-center justify-center hover:brightness-110 border"
            style={{
              background: canSend
                ? "var(--accent-primary)"
                : "color-mix(in srgb, var(--text-primary) 8%, transparent)",
              borderColor: canSend ? "transparent" : "var(--border-subtle)",
              color: canSend ? "var(--text-inverse)" : "var(--text-muted)",
              boxShadow: "none",
            }}
          >
            {isSending ? <LoadingSpinner /> : <SendIcon />}
          </button>
        </div>
      </div>

      {/* Attachment Gallery - compact variant, shown below textarea */}
      {attachments && attachments.length > 0 && (
        <div className="mt-2">
          <ChatAttachmentGallery
            attachments={attachments}
            {...(onRemoveAttachment !== undefined && { onRemove: onRemoveAttachment })}
            compact={true}
          />
        </div>
      )}

      {/* Footer - keyboard help and optional conversation persona confirmation */}
      {personaControl ? (
        <div
          data-testid="chat-input-footer"
          className="mt-1.5 flex min-w-0 items-center justify-between gap-2"
        >
          {showHelperText && (
            <p
              className="min-w-0 text-[0.625rem]"
              style={{ color: "var(--text-muted)" }}
            >
              {questionMode
                ? "Enter to send · Type option number or custom text"
                : hasQueuedMessages
                  ? "Enter to send · Shift+Enter for new line · ↑ to edit queued"
                  : "Enter to send · Shift+Enter for new line"}
            </p>
          )}
          <div
            data-testid="chat-input-persona-control"
            className="ml-auto min-w-0 shrink-0"
          >
            {personaControl}
          </div>
        </div>
      ) : showHelperText ? (
        <p
          className="text-[0.625rem] mt-1.5"
          style={{ color: "var(--text-muted)" }}
        >
          {questionMode
            ? "Enter to send · Type option number or custom text"
            : hasQueuedMessages
              ? "Enter to send · Shift+Enter for new line · ↑ to edit queued"
              : "Enter to send · Shift+Enter for new line"}
        </p>
      ) : null}

      {isAttachmentDragging && <ChatAttachmentDropOverlay roundedClassName="rounded-lg" />}
    </div>
  );
}
