/**
 * useProjectCreationWizard - step orchestration for the project creation dialog.
 *
 * Owns only step navigation, first-run/isCreating close gating, the
 * hand-off path passed from ExistingRepositoryStep's "switch to Create New"
 * recovery action into NewRepositoryStep, and the clone-active flag CloneStep
 * reports while a clone is running (Escape/backdrop must stay inert then;
 * Cancel Clone is the only exit). Each step component owns its own form
 * state locally since the intents have disjoint shapes.
 */

import { useCallback, useEffect, useState } from "react";

export type WizardStep = "intent" | "clone" | "create" | "existing";

export interface UseProjectCreationWizardArgs {
  isOpen: boolean;
  isFirstRun: boolean;
  isCreating: boolean;
  onClose: () => void;
}

export interface UseProjectCreationWizardResult {
  step: WizardStep;
  goToStep: (step: WizardStep) => void;
  goBackToIntent: () => void;
  handleOpenChange: (open: boolean) => void;
  /** Absolute path carried from a recovery action (e.g. Existing -> Create New). */
  handoffPath: string | null;
  goToCreateWithPath: (path: string) => void;
  /** Whether CloneStep currently has a clone job running. */
  cloningActive: boolean;
  setCloningActive: (active: boolean) => void;
}

export function useProjectCreationWizard({
  isOpen,
  isFirstRun,
  isCreating,
  onClose,
}: UseProjectCreationWizardArgs): UseProjectCreationWizardResult {
  const [step, setStep] = useState<WizardStep>("intent");
  const [handoffPath, setHandoffPath] = useState<string | null>(null);
  const [cloningActive, setCloningActive] = useState(false);

  // Reset to the intent chooser every time the dialog opens.
  useEffect(() => {
    if (isOpen) {
      setStep("intent");
      setHandoffPath(null);
      setCloningActive(false);
    }
  }, [isOpen]);

  const goToStep = useCallback((next: WizardStep) => {
    setStep(next);
  }, []);

  const goBackToIntent = useCallback(() => {
    setHandoffPath(null);
    setStep("intent");
  }, []);

  const goToCreateWithPath = useCallback((path: string) => {
    setHandoffPath(path);
    setStep("create");
  }, []);

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open && !isFirstRun && !isCreating && !cloningActive) {
        onClose();
      }
    },
    [isFirstRun, isCreating, cloningActive, onClose]
  );

  return {
    step,
    goToStep,
    goBackToIntent,
    handleOpenChange,
    handoffPath,
    goToCreateWithPath,
    cloningActive,
    setCloningActive,
  };
}
