import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { SizeBudgetPreview } from "@/api/data-retention";
import { formatBytes, formatTimestamp } from "./settings-bytes";

export interface DataRetentionSizeBudgetDialogProps {
  open: boolean;
  preview: SizeBudgetPreview | null;
  budgetBytes: number;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}

/**
 * Consent gate for size-based pruning. The user is authorizing deletion of tool-call
 * detail that is still inside the retention window, so the dialog states the concrete
 * measured outcome rather than a generic warning.
 */
export function DataRetentionSizeBudgetDialog({
  open, preview, budgetBytes, onOpenChange, onConfirm,
}: DataRetentionSizeBudgetDialogProps) {
  const deletesNothing = !preview || preview.rows === 0;
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            Limit tool-call detail to {formatBytes(budgetBytes)}?
          </AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="space-y-2" data-testid="size-budget-preview">
              {deletesNothing ? (
                <p>
                  Nothing is deleted right now — your stored tool-call detail is already
                  under {formatBytes(budgetBytes)}. RalphX will keep it under that limit
                  from now on by deleting the oldest detail first.
                </p>
              ) : (
                <p>
                  RalphX will delete about{" "}
                  <strong data-testid="size-budget-preview-rows">{preview.rows.toLocaleString()}</strong>{" "}
                  tool-call records (
                  <strong data-testid="size-budget-preview-bytes">{formatBytes(preview.bytes)}</strong>
                  ), everything recorded before{" "}
                  <strong data-testid="size-budget-preview-cut">{formatTimestamp(preview.cutCreatedAt)}</strong>.
                </p>
              )}
              <p>
                Message text, tool input and result previews, and the conversation timeline
                all stay. Only the full tool-call payloads are removed.
              </p>
              <p>This cannot be undone.</p>
            </div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={(event) => { event.preventDefault(); onConfirm(); }}
          >
            Enable size limit
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
