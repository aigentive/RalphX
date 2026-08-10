import { FolderOpen } from "lucide-react";

import { ComposerReferencePill } from "./ComposerReferencePill";

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
  removeDisabled?: boolean;
  removeDisabledReason?: string;
  testId?: string;
}

export function FolderReferenceChips<T extends FolderReferenceChipLike>({
  references,
  onRemove,
  removingId,
  removeDisabled = false,
  removeDisabledReason,
  testId = "folder-reference-chips",
}: FolderReferenceChipsProps<T>) {
  if (references.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-2 pb-3" data-testid={testId}>
      {references.map((reference) => (
        <ComposerReferencePill
          key={reference.id}
          testId={`agent-composer-reference-pill-folder:${reference.id}`}
          icon={FolderOpen}
          typeLabel="Folder"
          label={reference.displayName}
          description={reference.folderPath}
          contentTooltip={reference.folderPath}
          removeLabel={`Remove folder ${reference.displayName}`}
          removeDisabled={removeDisabled || removingId === reference.id}
          {...(removeDisabledReason ? { removeDisabledReason } : {})}
          onRemove={() => onRemove(reference)}
        />
      ))}
    </div>
  );
}
