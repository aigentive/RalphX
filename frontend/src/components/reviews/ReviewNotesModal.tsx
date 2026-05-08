/**
 * ReviewNotesModal - Modal for adding review notes with optional fix description
 * Used when approving/rejecting reviews or requesting changes
 *
 * Uses shadcn/ui Dialog, Textarea, Label, Button components.
 */

import { useState, useCallback, useEffect } from "react";
import { MessageSquare, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";

interface ReviewNotesModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (data: { notes: string; fixDescription?: string }) => void;
  title: string;
  showFixDescription?: boolean;
  notesLabel?: string;
  notesPlaceholder?: string;
  notesRequired?: boolean;
  isProcessing?: boolean;
}

export function ReviewNotesModal({
  isOpen,
  onClose,
  onSubmit,
  title,
  showFixDescription = false,
  notesLabel = "Notes",
  notesPlaceholder = "Enter your review notes...",
  notesRequired = false,
  isProcessing = false,
}: ReviewNotesModalProps) {
  const [notes, setNotes] = useState("");
  const [fixDescription, setFixDescription] = useState("");

  // Reset form when modal closes
  useEffect(() => {
    if (!isOpen) {
      setNotes("");
      setFixDescription("");
    }
  }, [isOpen]);

  const handleSubmit = useCallback(() => {
    if (isProcessing) return;

    const data: { notes: string; fixDescription?: string } = { notes };
    if (showFixDescription) {
      data.fixDescription = fixDescription;
    }
    onSubmit(data);
    setNotes("");
    setFixDescription("");
  }, [notes, fixDescription, showFixDescription, onSubmit, isProcessing]);

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open && !isProcessing) {
        setNotes("");
        setFixDescription("");
        onClose();
      }
    },
    [onClose, isProcessing]
  );

  const isSubmitDisabled = (notesRequired && notes.trim() === "") || isProcessing;

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        data-testid="review-notes-modal"
        data-has-fix-description={showFixDescription ? "true" : "false"}
        className="max-w-md"
        aria-describedby={undefined}
      >
        <DialogHeader>
          <div className="flex items-center gap-3">
            <MessageSquare className="w-5 h-5 text-[var(--accent-primary)]" />
            <DialogTitle data-testid="modal-title">{title}</DialogTitle>
          </div>
        </DialogHeader>

        <div
          data-testid="review-notes-modal-content"
          className="px-6 py-4 space-y-4"
          style={{ backgroundColor: "var(--bg-elevated)" }}
        >
          <div className="space-y-2">
            <Label
              htmlFor="review-notes"
              className="text-sm font-medium text-[var(--text-secondary)]"
            >
              {notesLabel}
              {notesRequired && <span className="text-[var(--status-error)] ml-1">*</span>}
            </Label>
            <Textarea
              id="review-notes"
              data-testid="notes-textarea"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={notesPlaceholder}
              rows={3}
              disabled={isProcessing}
              className="resize-none bg-[var(--bg-base)] border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]"
            />
          </div>

          {showFixDescription && (
            <div className="space-y-2">
              <Label
                htmlFor="fix-description"
                className="text-sm font-medium text-[var(--text-secondary)]"
              >
                Fix Description
              </Label>
              <Textarea
                id="fix-description"
                data-testid="fix-description-textarea"
                value={fixDescription}
                onChange={(e) => setFixDescription(e.target.value)}
                placeholder="Describe what needs to be fixed..."
                rows={3}
                disabled={isProcessing}
                className="resize-none bg-[var(--bg-base)] border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]"
              />
            </div>
          )}
        </div>

        <DialogFooter data-testid="cancel-button-container">
          <Button
            data-testid="cancel-button"
            variant="ghost"
            onClick={handleOpenChange.bind(null, false)}
            disabled={isProcessing}
            className="text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
          >
            Cancel
          </Button>
          <Button
            data-testid="confirm-button"
            onClick={handleSubmit}
            disabled={isSubmitDisabled}
            className="bg-[var(--status-success)] hover:bg-[var(--status-success)]/90 text-white active:scale-[0.98] transition-all"
          >
            {isProcessing && <Loader2 className="w-4 h-4 mr-2 animate-spin" />}
            {isProcessing ? "Submitting..." : "Submit"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
