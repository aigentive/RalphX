/**
 * ProjectCreationWizard - Modal for creating a new project
 *
 * Dialog shell + step router. Landing step is an explicit intent chooser
 * (Clone / Create New / Add Existing); each intent gets its own step
 * component that owns its own form state and submission flow.
 */

import type { CreateProject } from "@/types/project";
import type { ProjectCandidate } from "@/types/project-candidate";
import type {
  PrepareNewProjectDirectoryInput,
  PreparedProjectDirectory,
} from "@/api/projects";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ArrowLeft } from "lucide-react";
import { useProjectCreationWizard, type WizardStep } from "./useProjectCreationWizard";
import { IntentStep } from "./steps/IntentStep";
import { CloneStep } from "./steps/CloneStep";
import { ExistingRepositoryStep } from "./steps/ExistingRepositoryStep";
import { NewRepositoryStep } from "./steps/NewRepositoryStep";

// ============================================================================
// Props Interface
// ============================================================================

export interface ProjectCreationWizardProps {
  /** Whether the modal is open */
  isOpen: boolean;
  /** Callback when the modal is closed */
  onClose: () => void;
  /** Callback when a project is created */
  onCreate: (project: CreateProject) => void | Promise<void>;
  /** Callback to open a folder picker, returns selected path or null if cancelled */
  onBrowseFolder?: (options?: { title?: string }) => Promise<string | null>;
  /** Callback to inspect a candidate path before offering to register it */
  onInspectCandidate?: (path: string) => Promise<ProjectCandidate>;
  /** Callback to prepare (or accept) the destination directory for a new repository */
  onPrepareDirectory?: (
    input: PrepareNewProjectDirectoryInput
  ) => Promise<PreparedProjectDirectory>;
  /** Callback to roll back a prepared directory when project creation failed */
  onDiscardDirectory?: (path: string) => Promise<void>;
  /** Whether creation is in progress */
  isCreating?: boolean;
  /** Error message to display */
  error?: string | null;
  /** Whether this is the first-run mode (no existing projects) - disables close/cancel */
  isFirstRun?: boolean;
}

const STEP_TITLES: Record<WizardStep, string> = {
  intent: "Create New Project",
  clone: "Clone Repository",
  create: "Create New Repository",
  existing: "Add Existing Repository",
};

// ============================================================================
// Main Component
// ============================================================================

export function ProjectCreationWizard({
  isOpen,
  onClose,
  onCreate,
  onBrowseFolder,
  onInspectCandidate,
  onPrepareDirectory,
  onDiscardDirectory,
  isCreating = false,
  error = null,
  isFirstRun = false,
}: ProjectCreationWizardProps) {
  const {
    step,
    goToStep,
    goBackToIntent,
    handleOpenChange,
    handoffPath,
    goToCreateWithPath,
    cloningActive,
    setCloningActive,
  } = useProjectCreationWizard({ isOpen, isFirstRun, isCreating, onClose });

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        data-testid="project-creation-wizard"
        hideCloseButton={isFirstRun}
        className="max-w-lg p-0"
        aria-describedby={undefined}
        onPointerDownOutside={(e) => {
          if (isFirstRun || isCreating || cloningActive) e.preventDefault();
        }}
        onEscapeKeyDown={(e) => {
          if (isFirstRun || isCreating || cloningActive) e.preventDefault();
        }}
      >
        <DialogHeader className="px-6 py-4 border-b border-[var(--border-subtle)] justify-start gap-2">
          {step !== "intent" && (
            <TooltipProvider delayDuration={250}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    data-testid="wizard-back-button"
                    aria-label="Back to project creation options"
                    onClick={goBackToIntent}
                    disabled={isCreating || cloningActive}
                    className="flex h-6 w-6 items-center justify-center rounded text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] disabled:opacity-50"
                  >
                    <ArrowLeft className="h-4 w-4" />
                  </button>
                </TooltipTrigger>
                <TooltipContent>Back</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
          <DialogTitle className="text-lg font-semibold text-[var(--text-primary)] tracking-tight">
            {STEP_TITLES[step]}
          </DialogTitle>
        </DialogHeader>

        {step === "intent" && (
          <div className="px-6 py-5">
            <IntentStep onSelectIntent={goToStep} />
          </div>
        )}
        {step === "clone" && (
          <CloneStep
            onCreate={onCreate}
            onBrowseFolder={onBrowseFolder}
            onClose={onClose}
            isCreating={isCreating}
            isFirstRun={isFirstRun}
            error={error}
            onActiveChange={setCloningActive}
          />
        )}
        {step === "existing" && (
          <ExistingRepositoryStep
            onCreate={onCreate}
            onBrowseFolder={onBrowseFolder}
            onInspectCandidate={onInspectCandidate}
            onClose={onClose}
            onSwitchToCreateNew={goToCreateWithPath}
            isCreating={isCreating}
            isFirstRun={isFirstRun}
            error={error}
          />
        )}
        {step === "create" && (
          <NewRepositoryStep
            onCreate={onCreate}
            onBrowseFolder={onBrowseFolder}
            onInspectCandidate={onInspectCandidate}
            onPrepareDirectory={onPrepareDirectory}
            onDiscardDirectory={onDiscardDirectory}
            onClose={onClose}
            isCreating={isCreating}
            isFirstRun={isFirstRun}
            error={error}
            initialFolderName={handoffPath ? handoffPath.split("/").pop() ?? "" : ""}
            initialParentDirectory={
              handoffPath ? handoffPath.split("/").slice(0, -1).join("/") : ""
            }
          />
        )}
      </DialogContent>
    </Dialog>
  );
}
