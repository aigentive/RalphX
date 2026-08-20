/**
 * NewRepositoryStep - "Create New Repository" intent.
 *
 * Submits by preparing the destination directory first, then creating the
 * project. If `create_project` fails after RalphX created the directory, the
 * prepared directory is rolled back so no half-made folder survives.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CreateProject } from "@/types/project";
import type { ProjectCandidate } from "@/types/project-candidate";
import type {
  PrepareNewProjectDirectoryInput,
  PreparedProjectDirectory,
} from "@/api/projects";
import { DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { AlertTriangle, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { generateWorktreePath, deriveFolderName } from "../ProjectCreationWizard.helpers";
import { GitSettingsSection } from "../parts/GitSettingsSection";
import { DestinationPicker } from "../parts/DestinationPicker";

const PROBE_DEBOUNCE_MS = 300;

function destinationWarning(candidate: ProjectCandidate | null): string | null {
  if (!candidate) return null;
  switch (candidate.kind) {
    case "notADirectory":
      return "A file already exists at this location.";
    case "nonEmptyNonRepo":
      return "This folder already has contents. Choose an empty or new folder name.";
    case "nestedInRepository":
      return `This location is inside another repository (${candidate.repositoryRoot}).`;
    case "repository":
      return "A repository already exists at this location.";
    default:
      return null;
  }
}

export interface NewRepositoryStepProps {
  onCreate: (project: CreateProject) => Promise<void> | void;
  onBrowseFolder?: ((options?: { title?: string }) => Promise<string | null>) | undefined;
  onInspectCandidate?: ((path: string) => Promise<ProjectCandidate>) | undefined;
  onPrepareDirectory?:
    | ((input: PrepareNewProjectDirectoryInput) => Promise<PreparedProjectDirectory>)
    | undefined;
  onDiscardDirectory?: ((path: string) => Promise<void>) | undefined;
  onClose: () => void;
  isCreating: boolean;
  isFirstRun?: boolean | undefined;
  error?: string | null | undefined;
  initialParentDirectory?: string | undefined;
  initialFolderName?: string | undefined;
}

export function NewRepositoryStep({
  onCreate,
  onBrowseFolder,
  onInspectCandidate,
  onPrepareDirectory,
  onDiscardDirectory,
  onClose,
  isCreating,
  isFirstRun = false,
  error = null,
  initialParentDirectory = "",
  initialFolderName = "",
}: NewRepositoryStepProps) {
  const [parentDirectory, setParentDirectory] = useState(initialParentDirectory);
  const [folderName, setFolderName] = useState(initialFolderName);
  const [isFolderNameManuallySet, setIsFolderNameManuallySet] = useState(
    initialFolderName.length > 0
  );
  const [name, setName] = useState("");
  const [baseBranch, setBaseBranch] = useState("main");
  const [worktreeParentDirectory, setWorktreeParentDirectory] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [candidate, setCandidate] = useState<ProjectCandidate | null>(null);
  const probeGeneration = useRef(0);

  const worktreePath = useMemo(
    () => generateWorktreePath(folderName, worktreeParentDirectory),
    [folderName, worktreeParentDirectory]
  );

  useEffect(() => {
    if (!isFolderNameManuallySet && name) {
      setFolderName(deriveFolderName(name));
    }
  }, [name, isFolderNameManuallySet]);

  useEffect(() => {
    const destination = parentDirectory && folderName ? `${parentDirectory}/${folderName}` : "";
    if (!destination || !onInspectCandidate) {
      setCandidate(null);
      return;
    }
    const generation = ++probeGeneration.current;
    const timer = setTimeout(() => {
      onInspectCandidate(destination)
        .then((result) => {
          if (probeGeneration.current === generation) setCandidate(result);
        })
        .catch(() => {
          if (probeGeneration.current === generation) setCandidate(null);
        });
    }, PROBE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [parentDirectory, folderName, onInspectCandidate]);

  const warning = destinationWarning(candidate);
  const missingRequired = !parentDirectory.trim() || !folderName.trim() || !baseBranch.trim();
  const hasErrors = missingRequired || warning !== null;

  const handleBrowseParent = useCallback(async () => {
    if (!onBrowseFolder) return;
    const path = await onBrowseFolder({ title: "Select Parent Folder" });
    if (path) setParentDirectory(path);
  }, [onBrowseFolder]);

  const handleFolderNameChange = useCallback((value: string) => {
    setIsFolderNameManuallySet(true);
    setFolderName(value);
  }, []);

  const handleSubmit = useCallback(async () => {
    setSubmitted(true);
    setLocalError(null);
    if (!parentDirectory.trim() || !folderName.trim() || !baseBranch.trim() || warning) return;
    if (!onPrepareDirectory) return;

    let prepared: PreparedProjectDirectory | null = null;
    try {
      prepared = await onPrepareDirectory({
        parentDirectory: parentDirectory.trim(),
        folderName: folderName.trim(),
      });
      const projectName = name.trim() || folderName.trim();
      await onCreate({
        name: projectName,
        workingDirectory: prepared.path,
        gitMode: "worktree",
        baseBranch: baseBranch.trim(),
        worktreeParentDirectory: worktreeParentDirectory.trim() || "~/ralphx-worktrees",
      });
    } catch (creationError) {
      if (prepared?.created && onDiscardDirectory) {
        await onDiscardDirectory(prepared.path).catch(() => {
          // Best-effort rollback; the surfaced error already explains the failure.
        });
      }
      if (!prepared) {
        setLocalError(
          creationError instanceof Error ? creationError.message : "Failed to prepare the destination folder"
        );
      }
    }
  }, [
    parentDirectory,
    folderName,
    baseBranch,
    worktreeParentDirectory,
    name,
    warning,
    onPrepareDirectory,
    onCreate,
    onDiscardDirectory,
  ]);

  const displayError = error ?? localError;

  return (
    <>
      <div className="px-6 py-5 space-y-5">
        <DestinationPicker
          parentDirectory={parentDirectory}
          onParentDirectoryChange={setParentDirectory}
          folderName={folderName}
          onFolderNameChange={handleFolderNameChange}
          onBrowseParent={onBrowseFolder ? handleBrowseParent : undefined}
          isCreating={isCreating}
          parentError={submitted && !parentDirectory.trim() ? "Parent folder is required" : undefined}
          folderNameError={submitted && !folderName.trim() ? "Folder name is required" : undefined}
        />

        {warning && (
          <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-warning-muted)] text-[var(--status-warning)]">
            <AlertTriangle className="h-3.5 w-3.5" />
            <span className="text-sm">{warning}</span>
          </div>
        )}

        <div className="space-y-1.5">
          <Label
            htmlFor="project-name-input"
            className="text-sm font-medium text-[var(--text-secondary)]"
          >
            Project Name <span className="text-[var(--text-muted)]">(optional)</span>
          </Label>
          <Input
            id="project-name-input"
            data-testid="project-name-input"
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={folderName || "my-app"}
            disabled={isCreating}
            className="h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]"
          />
        </div>

        <Separator className="bg-[var(--border-subtle)]" />

        <GitSettingsSection
          baseBranchMode="freeText"
          baseBranchLabel="Starting branch"
          baseBranch={baseBranch}
          onBaseBranchChange={setBaseBranch}
          baseBranchError={submitted && !baseBranch.trim() ? "Starting branch is required" : undefined}
          baseBranchTouched={submitted}
          worktreePath={worktreePath}
          worktreeParentDirectory={worktreeParentDirectory}
          onWorktreeParentDirectoryChange={setWorktreeParentDirectory}
          showAdvanced={showAdvanced}
          onShowAdvancedChange={setShowAdvanced}
          isCreating={isCreating}
        />

        {displayError && (
          <div
            data-testid="wizard-error"
            className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-error-muted)] text-[var(--status-error)]"
          >
            <AlertTriangle className="h-3.5 w-3.5" />
            <span className="text-sm">{displayError}</span>
          </div>
        )}
      </div>

      <DialogFooter className="px-6 py-4 border-t border-[var(--border-subtle)] gap-3 sm:gap-3">
        {!isFirstRun && !isCreating && (
          <span className="mr-auto text-xs text-[var(--text-muted)]">
            Press <kbd className="px-1.5 py-0.5 rounded bg-[var(--bg-base)] border border-[var(--border-subtle)] font-mono text-[0.625rem]">ESC</kbd> to cancel
          </span>
        )}
        {!isFirstRun && (
          <Button
            data-testid="cancel-button"
            type="button"
            onClick={onClose}
            disabled={isCreating}
            variant="ghost"
            className="bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
          >
            Cancel
          </Button>
        )}
        <Button
          data-testid="create-button"
          type="button"
          onClick={handleSubmit}
          disabled={isCreating || (submitted && hasErrors)}
          className={cn(
            "gap-2",
            isCreating || (submitted && hasErrors)
              ? "bg-[var(--bg-hover)] text-[var(--text-muted)] cursor-not-allowed"
              : "bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-primary)]/90"
          )}
        >
          {isCreating && <Loader2 className="h-4 w-4 animate-spin" />}
          {isCreating ? "Creating..." : "Create Project"}
        </Button>
      </DialogFooter>
    </>
  );
}
