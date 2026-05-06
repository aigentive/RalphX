import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import {
  buildReviewFeedbackPreview,
  sanitizeReviewFeedbackText,
} from "@/lib/review-feedback";

interface ReviewFeedbackBodyProps {
  summary?: string | null;
  notes?: string | null;
  mode?: "markdown" | "plain";
  previewCharLimit?: number;
  /**
   * Bodies up to this size expand inline when the user clicks the toggle.
   * Bodies larger than this open in the dialog instead.
   */
  inlineExpandLimit?: number;
  dialogTitle?: string;
  dialogDescription?: string;
  fullButtonLabel?: string;
  collapseButtonLabel?: string;
  fullButtonClassName?: string;
  previewClassName?: string;
  dialogBodyClassName?: string;
}

// Pull lists flush with the section edge so review bullets line up with the
// surrounding heading and paragraph text instead of sitting under an indent.
const flushListComponents = {
  ...markdownComponents,
  ul: ({ children, ...props }: React.HTMLAttributes<HTMLUListElement>) => (
    <ul className="list-disc pl-5 -ml-5 mb-2" {...props}>
      {children}
    </ul>
  ),
  ol: ({ children, ...props }: React.HTMLAttributes<HTMLOListElement>) => (
    <ol className="list-decimal pl-5 -ml-5 mb-2" {...props}>
      {children}
    </ol>
  ),
};

function MarkdownBody({ content, className }: { content: string; className?: string }) {
  return (
    <div
      className={cn(
        "prose prose-sm prose-invert max-w-none",
        // Tailwind Typography wraps inline code with backtick pseudo-elements;
        // the markdown parser already strips delimiters, so disable both.
        "prose-code:before:content-none prose-code:after:content-none",
        className,
      )}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={flushListComponents}>
        {content}
      </ReactMarkdown>
    </div>
  );
}

export function ReviewFeedbackBody({
  summary,
  notes,
  mode = "markdown",
  previewCharLimit = 900,
  inlineExpandLimit = 4000,
  dialogTitle = "Full feedback",
  dialogDescription = "Full feedback in a scrollable view.",
  fullButtonLabel = "View full details",
  collapseButtonLabel = "Show less",
  fullButtonClassName,
  previewClassName,
  dialogBodyClassName,
}: ReviewFeedbackBodyProps) {
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const [isExpandedInline, setIsExpandedInline] = useState(false);

  const sanitizedSummary = useMemo(
    () => (summary ? sanitizeReviewFeedbackText(summary) : null),
    [summary]
  );
  const sanitizedNotes = useMemo(
    () => (notes ? sanitizeReviewFeedbackText(notes) : null),
    [notes]
  );

  const previewText = useMemo(() => {
    if (!sanitizedNotes) {
      return null;
    }
    return buildReviewFeedbackPreview(sanitizedNotes, previewCharLimit);
  }, [previewCharLimit, sanitizedNotes]);
  const isNotesTruncated = useMemo(() => {
    if (!sanitizedNotes) return false;
    return sanitizedNotes.length > previewCharLimit;
  }, [previewCharLimit, sanitizedNotes]);

  const showSummary = !!sanitizedSummary;
  const showFullBody = !!sanitizedNotes;
  const renderToggle = showFullBody && isNotesTruncated;
  const expandsToDialog =
    renderToggle && (sanitizedNotes?.length ?? 0) > inlineExpandLimit;
  const notesIncludeSummary =
    showSummary &&
    showFullBody &&
    sanitizedNotes
      .replace(/\s+/g, " ")
      .trim()
      .startsWith(sanitizedSummary.replace(/\s+/g, " ").trim());

  const fullInlineBody = showFullBody
    ? showSummary && !notesIncludeSummary
      ? `${sanitizedSummary}\n\n${sanitizedNotes}`
      : sanitizedNotes
    : null;

  const showFullInline =
    isExpandedInline && renderToggle && !expandsToDialog && fullInlineBody;

  const previewBody = showFullInline
    ? fullInlineBody
    : showFullBody && !isNotesTruncated
    ? showSummary && !notesIncludeSummary
      ? `${sanitizedSummary}\n\n${sanitizedNotes}`
      : sanitizedNotes
    : showSummary
    ? sanitizedSummary
    : showFullBody
    ? previewText
    : null;

  if (!previewBody) {
    return null;
  }

  const handleToggle = () => {
    if (expandsToDialog) {
      setIsDialogOpen(true);
      return;
    }
    setIsExpandedInline((prev) => !prev);
  };

  const buttonLabel = showFullInline ? collapseButtonLabel : fullButtonLabel;

  return (
    <>
      <div className={previewClassName}>
        {mode === "markdown" ? (
          <MarkdownBody content={previewBody} />
        ) : mode === "plain" ? (
          <pre className="whitespace-pre-wrap break-words font-inherit">
            {previewBody}
          </pre>
        ) : (
          <div className="whitespace-pre-wrap break-words">{previewBody}</div>
        )}
      </div>

      {renderToggle && (
        <>
          <button
            type="button"
            className={cn(
              "mt-2 font-medium text-[var(--accent-primary)] hover:text-[var(--accent-primary-hover)]",
              fullButtonClassName ?? "text-[0.75rem]"
            )}
            onClick={handleToggle}
          >
            {buttonLabel}
          </button>
          {expandsToDialog && (
            <Dialog open={isDialogOpen} onOpenChange={setIsDialogOpen}>
              <DialogContent className="sm:max-w-3xl max-h-[80vh] overflow-hidden">
                <DialogHeader>
                  <DialogTitle>{dialogTitle}</DialogTitle>
                  <DialogDescription>{dialogDescription}</DialogDescription>
                </DialogHeader>
                <div className="px-6 pb-6">
                  <div
                    className={cn(
                      "max-h-[56vh] overflow-y-auto rounded-lg bg-[var(--overlay-faint)] p-4",
                      dialogBodyClassName
                    )}
                  >
                    {mode === "markdown" ? (
                      <MarkdownBody content={sanitizedNotes} />
                    ) : (
                      <pre className="whitespace-pre-wrap break-words font-mono text-[0.75rem] text-text-primary/80">
                        {sanitizedNotes}
                      </pre>
                    )}
                  </div>
                </div>
              </DialogContent>
            </Dialog>
          )}
        </>
      )}
    </>
  );
}
