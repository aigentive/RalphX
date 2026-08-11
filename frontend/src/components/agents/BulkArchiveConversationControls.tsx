import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";

interface BulkArchiveConversationControlsProps {
  confirmationOpen: boolean;
  onCancel: () => void;
  onCloseConfirmation: () => void;
  onConfirm: () => Promise<void>;
  onOpenConfirmation: () => void;
  onMute: () => void;
  pending: boolean;
  selectedCount: number;
}

export function BulkArchiveConversationControls({
  confirmationOpen,
  onCancel,
  onCloseConfirmation,
  onConfirm,
  onOpenConfirmation,
  onMute,
  pending,
  selectedCount,
}: BulkArchiveConversationControlsProps) {
  return (
    <>
      <div
        className="mx-3 mb-2 flex shrink-0 items-center gap-2 rounded-[6px] px-2 py-1.5"
        role="region"
        aria-label="Bulk archive actions"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <span
          className="mr-auto text-[0.7188rem] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          {selectedCount} selected
        </span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={onCancel}
          disabled={pending}
        >
          Cancel
        </Button>
        <Button
          type="button"
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={onMute}
          disabled={selectedCount === 0 || pending}
        >
          Mute
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs"
          style={{ color: "var(--destructive)" }}
          onClick={onOpenConfirmation}
          disabled={selectedCount === 0 || pending}
        >
          Archive selected
        </Button>
      </div>

      <AlertDialog
        open={confirmationOpen}
        onOpenChange={(open) => {
          if (!open) {
            onCloseConfirmation();
          }
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Archive selected sessions?</AlertDialogTitle>
            <AlertDialogDescription>
              This hides {selectedCount} selected {selectedCount === 1 ? "session" : "sessions"}
              {" "}from the active conversation list and permanently deletes each local RalphX
              workspace and local branch, including uncommitted changes and ignored build or test
              artifacts. Remote pull requests remain open.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={selectedCount === 0 || pending}
              onClick={(event) => {
                event.preventDefault();
                void onConfirm();
              }}
            >
              {pending ? "Archiving selected..." : "Archive selected"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
