import { FolderOpen, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

/** Minimal shape shared by hydrated `ConversationFolderReference`s and
 * pre-send draft-local `ChatComposerFolder`s so this row can render either. */
export interface FolderReferenceChipLike {
  id: string;
  folderPath: string;
  displayName: string;
}

interface FolderReferenceChipsProps<T extends FolderReferenceChipLike> {
  references: T[];
  onRemove: (reference: T) => void;
  removingId?: string;
  testId?: string;
}

export function FolderReferenceChips<T extends FolderReferenceChipLike>({
  references,
  onRemove,
  removingId,
  testId = "folder-reference-chips",
}: FolderReferenceChipsProps<T>) {
  if (references.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-2 pb-3" data-testid={testId}>
      {references.map((reference) => (
        <Tooltip key={reference.id}>
          <TooltipTrigger asChild>
            <div className="flex items-center gap-1 rounded-md border px-2 py-1 text-xs" style={{ borderColor: "var(--border-subtle)", color: "var(--text-primary)" }}>
              <FolderOpen className="h-3.5 w-3.5" aria-hidden="true" />
              <span>{reference.displayName}</span>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-5 w-5"
                    aria-label={`Remove folder ${reference.displayName}`}
                    disabled={removingId === reference.id}
                    onClick={() => onRemove(reference)}
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Remove folder</TooltipContent>
              </Tooltip>
            </div>
          </TooltipTrigger>
          <TooltipContent>{reference.folderPath}</TooltipContent>
        </Tooltip>
      ))}
    </div>
  );
}
