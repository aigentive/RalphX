/**
 * IntentStep - the project-creation intent chooser (Clone / Create New / Add
 * Existing). Selecting a card immediately advances to that step.
 */

import { Download, FolderPlus, FolderSearch } from "lucide-react";
import { RadioOption } from "../ProjectCreationWizard.components";
import type { WizardStep } from "../useProjectCreationWizard";

export interface IntentStepProps {
  onSelectIntent: (step: Extract<WizardStep, "clone" | "create" | "existing">) => void;
}

const INTENT_GROUP_NAME = "project-intent";

export function IntentStep({ onSelectIntent }: IntentStepProps) {
  return (
    <div className="space-y-2" role="radiogroup" aria-label="Project creation method">
      <RadioOption
        testId="intent-clone"
        name={INTENT_GROUP_NAME}
        value="clone"
        selected={false}
        onSelect={() => onSelectIntent("clone")}
        label="Clone Repository"
        description="Copy an existing remote repository (GitHub, GitLab, or any git URL) onto your machine."
        icon={<Download className="h-4 w-4" style={{ color: "var(--accent-primary)" }} />}
      />
      <RadioOption
        testId="intent-create"
        name={INTENT_GROUP_NAME}
        value="create"
        selected={false}
        onSelect={() => onSelectIntent("create")}
        label="Create New Repository"
        description="Start a brand-new project in an empty folder."
        icon={<FolderPlus className="h-4 w-4" style={{ color: "var(--accent-primary)" }} />}
      />
      <RadioOption
        testId="intent-existing"
        name={INTENT_GROUP_NAME}
        value="existing"
        selected={false}
        onSelect={() => onSelectIntent("existing")}
        label="Add Existing Repository"
        description="Point RalphX at a folder you already have set up with version control."
        icon={<FolderSearch className="h-4 w-4" style={{ color: "var(--accent-primary)" }} />}
      />
    </div>
  );
}
