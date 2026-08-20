/**
 * DestinationPicker - parent folder + folder name inputs with a live path
 * preview, shared by NewRepositoryStep and CloneStep to choose where a
 * repository will land.
 *
 * `idPrefix` keeps element ids and test ids distinct per step so the two callers
 * never collide.
 */

import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

export interface DestinationPickerProps {
  parentDirectory: string;
  onParentDirectoryChange: (value: string) => void;
  folderName: string;
  onFolderNameChange: (value: string) => void;
  onBrowseParent?: (() => void) | undefined;
  isCreating: boolean;
  parentError?: string | undefined;
  folderNameError?: string | undefined;
  /** Namespaces element/test ids. Defaults to the Create New step's ids. */
  idPrefix?: string;
}

export function DestinationPicker({
  parentDirectory,
  onParentDirectoryChange,
  folderName,
  onFolderNameChange,
  onBrowseParent,
  isCreating,
  parentError,
  folderNameError,
  idPrefix = "new-project",
}: DestinationPickerProps) {
  const previewPath = parentDirectory && folderName ? `${parentDirectory}/${folderName}` : "";
  const parentId = `${idPrefix}-parent-input`;
  const folderNameId = `${idPrefix}-folder-name-input`;

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <Label
          htmlFor={parentId}
          className="text-sm font-medium text-[var(--text-secondary)]"
        >
          Parent Folder <span className="text-[var(--status-error)]">*</span>
        </Label>
        <div className="flex gap-2">
          <Input
            id={parentId}
            data-testid={parentId}
            type="text"
            value={parentDirectory}
            onChange={(e) => onParentDirectoryChange(e.target.value)}
            placeholder="Select a parent folder..."
            readOnly
            disabled={isCreating}
            className={cn(
              "flex-1 h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
              parentError ? "border-[var(--status-error)]" : "border-[var(--border-subtle)]",
              isCreating && "opacity-50"
            )}
          />
          {onBrowseParent && (
            <Button
              data-testid="browse-parent-button"
              type="button"
              onClick={onBrowseParent}
              disabled={isCreating}
              variant="secondary"
              className="h-10 px-3 gap-2 bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
            >
              <FolderOpen className="h-4 w-4" />
              Browse
            </Button>
          )}
        </div>
        {parentError && (
          <p
            data-testid={`${parentId}-error`}
            className="text-xs text-[var(--status-error)]"
          >
            {parentError}
          </p>
        )}
      </div>

      <div className="space-y-1.5">
        <Label
          htmlFor={folderNameId}
          className="text-sm font-medium text-[var(--text-secondary)]"
        >
          Folder Name <span className="text-[var(--status-error)]">*</span>
        </Label>
        <Input
          id={folderNameId}
          data-testid={folderNameId}
          type="text"
          value={folderName}
          onChange={(e) => onFolderNameChange(e.target.value)}
          placeholder="my-app"
          disabled={isCreating}
          className={cn(
            "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
            folderNameError ? "border-[var(--status-error)]" : "border-[var(--border-subtle)]",
            isCreating && "opacity-50"
          )}
        />
        {folderNameError && (
          <p
            data-testid={`${folderNameId}-error`}
            className="text-xs text-[var(--status-error)]"
          >
            {folderNameError}
          </p>
        )}
      </div>

      {previewPath && (
        <div
          data-testid={`${idPrefix}-destination-preview`}
          className="px-3 py-2 rounded-lg bg-[var(--bg-base)] text-sm truncate text-[var(--text-secondary)]"
        >
          {previewPath}
        </div>
      )}
    </div>
  );
}
