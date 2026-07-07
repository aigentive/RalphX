/**
 * QuestionInputBanner - Inline question UI above chat input
 *
 * Replaces the standalone AskUserQuestionCard. Renders as a banner
 * above the ChatInput with numbered option chips, slide-in/out
 * animations, and an answered collapsed state.
 *
 * Supports single-select (chip click selects one, dims others) and
 * multi-select (chip click toggles, checkmarks shown) modes.
 */

import { useState, useCallback, useEffect } from "react";
import { Check, CheckCircle2, Loader2, X, Maximize2, Minimize2 } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";
import { computeQuestionHeight } from "./QuestionInputBanner.utils";
import { markdownComponents } from "./MessageItem.markdown";
import { statusTint } from "@/lib/theme-colors";

// ============================================================================
// Types
// ============================================================================

export interface QuestionInputBannerProps {
  /** The active question payload (null when showing answered-only state) */
  question: AskUserQuestionPayload | null;
  /** Currently selected option indices (controlled from parent for input sync) */
  selectedIndices: Set<number>;
  /** Called when user clicks a chip */
  onChipClick: (index: number) => void;
  /** Called when user explicitly skips an optional question */
  onSkip?: () => void;
  /** Called when user clicks the dismiss (X) button */
  onDismiss: () => void;
  /** When set, shows the collapsed answered state */
  answeredValue?: string | undefined;
  /** Called when user clicks dismiss on the answered banner */
  onDismissAnswered?: (() => void) | undefined;
  /** Optional inline action shown alongside the question. */
  planApprovalAction?: {
    label: string;
    pendingLabel?: string;
    onClick: () => void;
    disabled?: boolean;
    isPending?: boolean;
  } | undefined;
}

// ============================================================================
// Sub-components
// ============================================================================

function OptionChip({
  index,
  label,
  selected,
  dimmed,
  multiSelect,
  onClick,
}: {
  index: number;
  label: string;
  selected: boolean;
  dimmed: boolean;
  multiSelect: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex items-center gap-1.5 rounded-full text-xs font-medium transition-all"
      style={{
        padding: "5px 12px 5px 8px",
        border: selected
          ? `1px solid ${statusTint("accent", 40)}`
          : "1px solid var(--overlay-moderate)",
        background: selected
          ? "var(--accent-muted)"
          : "var(--bg-surface)",
        color: "var(--text-primary)",
        opacity: dimmed ? 0.45 : 1,
        cursor: "pointer",
        userSelect: "none",
      }}
    >
      {/* Number circle */}
      <span
        className="inline-flex items-center justify-center flex-shrink-0 text-[0.625rem] font-bold"
        style={{
          width: 18,
          height: 18,
          borderRadius: "50%",
          background: selected
            ? "var(--accent-primary)"
            : "var(--overlay-weak)",
          color: selected ? "#fff" : "var(--text-muted)",
          transition: "all 0.15s ease",
        }}
      >
        {index + 1}
      </span>

      {/* Checkmark for multi-select */}
      {multiSelect && (
        <span
          className="text-[0.625rem]"
          style={{
            color: selected ? "var(--accent-primary)" : "var(--text-muted)",
            marginLeft: -2,
          }}
        >
          <Check size={10} strokeWidth={2.5} />
        </span>
      )}

      {/* Label */}
      <span className="font-medium">{label}</span>
    </button>
  );
}

function PlanApprovalActionButton({
  action,
}: {
  action: NonNullable<QuestionInputBannerProps["planApprovalAction"]>;
}) {
  const isDisabled = Boolean(action.disabled || action.isPending);

  return (
    <button
      type="button"
      onClick={action.onClick}
      disabled={isDisabled}
      data-testid="question-plan-approval-action"
      className="inline-flex h-7 flex-shrink-0 items-center gap-1.5 rounded-md px-2.5 text-[0.6875rem] font-semibold transition-colors"
      style={{
        border: "1px solid var(--accent-border)",
        background: "var(--accent-muted)",
        color: "var(--accent-primary)",
        cursor: isDisabled ? "not-allowed" : "pointer",
        opacity: isDisabled ? 0.65 : 1,
      }}
    >
      {action.isPending ? (
        <Loader2 className="h-3 w-3 animate-spin" />
      ) : (
        <CheckCircle2 className="h-3 w-3" />
      )}
      {action.isPending ? (action.pendingLabel ?? "Approving...") : action.label}
    </button>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export function QuestionInputBanner({
  question,
  selectedIndices,
  onChipClick,
  onSkip,
  onDismiss,
  answeredValue,
  onDismissAnswered,
  planApprovalAction,
}: QuestionInputBannerProps) {
  const [visible, setVisible] = useState(false);
  const [computedHeight, setComputedHeight] = useState(320);
  const [isExpanded, setIsExpanded] = useState(true);

  // Trigger slide-in on mount
  useEffect(() => {
    const raf = requestAnimationFrame(() => {
      setVisible(true);
    });
    return () => cancelAnimationFrame(raf);
  }, []);

  // Compute height when question changes, and reset expanded state
  useEffect(() => {
    if (question) {
      setComputedHeight(computeQuestionHeight(question));
      setIsExpanded(true); // Reset expanded state on new question (default to expanded)
    }
  }, [question]); // Re-compute when question object changes

  const handleDismiss = useCallback(() => {
    setVisible(false);
    // Wait for animation to complete before calling parent dismiss
    setTimeout(onDismiss, 350);
  }, [onDismiss]);

  const handleDismissAnswered = useCallback(() => {
    if (!onDismissAnswered) return;
    setVisible(false);
    setTimeout(onDismissAnswered, 350);
  }, [onDismissAnswered]);

  const isAnswered = answeredValue !== undefined;
  const showExpandButton = computedHeight >= 280;
  const canSkip = Boolean(question && question.allowSkip !== false && onSkip);
  const progressLabel = question?.batchIndex && question?.batchTotal && question.batchTotal > 1
    ? `${question.batchIndex} of ${question.batchTotal}`
    : null;

  // Nothing to show: no active question and no answered state
  if (!question && !isAnswered) return null;

  return (
    <div
      data-testid="question-input-banner"
      style={{
        overflow: "hidden",
        maxHeight: !visible
          ? 0
          : isAnswered
            ? 56
            : isExpanded
              ? "60vh"
              : computedHeight,
        opacity: visible ? 1 : 0,
        transition: `max-height 0.35s cubic-bezier(0.22, 1, 0.36, 1),
                     opacity 0.25s ease,
                     padding 0.35s cubic-bezier(0.22, 1, 0.36, 1)`,
        padding: visible ? "12px 12px 0" : "0 12px",
      }}
    >
      <div
        style={{
          background: "var(--bg-surface)",
          border: "1px solid var(--overlay-moderate)",
          borderRadius: 8,
          overflow: "hidden",
        }}
      >
        {isAnswered ? (
          /* ── Answered/collapsed state ── */
          <div
            data-testid="question-input-banner-answered"
            className="flex items-center gap-2"
            style={{ padding: "10px 12px" }}
          >
            {/* Success icon */}
            <span
              className="flex items-center justify-center flex-shrink-0"
              style={{
                width: 18,
                height: 18,
                borderRadius: "50%",
                background: "var(--status-success-muted)",
                color: "var(--status-success)",
                fontSize: 10,
              }}
            >
              <Check size={10} strokeWidth={3} />
            </span>

            <span className="text-xs" style={{ color: "var(--text-muted)" }}>
              Answered:
            </span>
            <span
              className="text-xs font-semibold truncate flex-1 min-w-0"
              style={{ color: "var(--text-primary)" }}
            >
              {answeredValue}
            </span>

            {planApprovalAction && (
              <PlanApprovalActionButton action={planApprovalAction} />
            )}

            {onDismissAnswered && (
              <button
                type="button"
                onClick={handleDismissAnswered}
                className="flex-shrink-0 flex items-center justify-center transition-colors"
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: 4,
                  border: "none",
                  background: "transparent",
                  color: "var(--text-muted)",
                  cursor: "pointer",
                }}
                aria-label="Dismiss answered summary"
              >
                <X size={12} />
              </button>
            )}
          </div>
        ) : question ? (
          /* ── Active question state ── */
          <>
            {/* Header row */}
            <div
              className="flex items-center gap-2"
              style={{
                padding: "8px 12px",
                borderBottom: "1px solid var(--overlay-weak)",
              }}
            >
              {/* ? icon in circle */}
              <span
                className="flex items-center justify-center flex-shrink-0 text-[0.6875rem] font-bold"
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: "50%",
                  background: "var(--accent-muted)",
                  color: "var(--accent-primary)",
                }}
              >
                ?
              </span>

              <span className="flex min-w-0 flex-1 items-center gap-1.5">
                <span
                  className="truncate text-[0.6875rem] font-semibold"
                  style={{ color: "var(--text-muted)" }}
                >
                  {question.header ?? "Question from agent"}
                </span>
                {progressLabel && (
                  <span
                    className="flex-shrink-0 rounded px-1.5 py-0.5 text-[0.625rem] font-semibold"
                    style={{
                      background: "var(--overlay-weak)",
                      color: "var(--text-muted)",
                    }}
                  >
                    {progressLabel}
                  </span>
                )}
              </span>

              {planApprovalAction && (
                <PlanApprovalActionButton action={planApprovalAction} />
              )}

              {canSkip && (
                <button
                  type="button"
                  onClick={onSkip}
                  className="flex-shrink-0 rounded-md px-2 py-1 text-[0.6875rem] font-semibold transition-colors"
                  style={{
                    border: "1px solid var(--overlay-moderate)",
                    background: "var(--bg-surface)",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                  }}
                >
                  Skip
                </button>
              )}

              {/* Expand/collapse button - only shown when content is near clipping threshold */}
              {showExpandButton && (
                <button
                  type="button"
                  onClick={() => setIsExpanded(!isExpanded)}
                  className="flex-shrink-0 flex items-center justify-center transition-colors"
                  style={{
                    width: 20,
                    height: 20,
                    borderRadius: 4,
                    border: "none",
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                  }}
                  aria-label={isExpanded ? "Collapse question" : "Expand question"}
                >
                  {isExpanded ? (
                    <Minimize2 size={16} />
                  ) : (
                    <Maximize2 size={16} />
                  )}
                </button>
              )}

              <button
                type="button"
                onClick={handleDismiss}
                className="flex-shrink-0 flex items-center justify-center transition-all"
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: 4,
                  border: "none",
                  background: "transparent",
                  color: "var(--text-muted)",
                  cursor: "pointer",
                }}
                aria-label="Dismiss question"
              >
                <X size={14} />
              </button>
            </div>

            {/* Body: question text + chips */}
            <div
              style={{
                padding: "10px 12px 12px",
                maxHeight: isExpanded ? "calc(60vh - 40px)" : undefined,
                overflowY: isExpanded ? "auto" : undefined,
              }}
            >
              <div
                className="text-[0.8125rem] font-medium leading-snug [&>p]:mb-0 [&>ul]:mb-0 [&>ol]:mb-0"
                style={{
                  color: "var(--text-primary)",
                  marginBottom: 10,
                  lineHeight: 1.45,
                }}
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                  {question.question}
                </ReactMarkdown>
              </div>

              {/* Option chips */}
              <div className="flex flex-wrap gap-1.5">
                {question.options.map((option, i) => {
                  const isSelected = selectedIndices.has(i);
                  const isDimmed = !question.multiSelect &&
                    selectedIndices.size > 0 &&
                    !isSelected;

                  return (
                    <OptionChip
                      key={option.value ?? option.label}
                      index={i}
                      label={option.label}
                      selected={isSelected}
                      dimmed={isDimmed}
                      multiSelect={question.multiSelect}
                      onClick={() => onChipClick(i)}
                    />
                  );
                })}
              </div>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
