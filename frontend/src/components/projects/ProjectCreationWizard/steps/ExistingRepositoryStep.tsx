/**
 * ExistingRepositoryStep - "Add Existing Repository" intent.
 *
 * Debounces a read-only probe of the selected folder with a generation
 * counter so a slow/stale probe response can never overwrite a newer one.
 * Probe failure degrades to "you can still continue" and never blocks.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CreateProject } from "@/types/project";
import type { ProjectCandidate } from "@/types/project-candidate";
import { DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { AlertTriangle, FolderOpen, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";
import {
  type FormState,
  extractFolderName,
  generateWorktreePath,
  validateForm,
} from "../ProjectCreationWizard.helpers";
import { GitSettingsSection } from "../parts/GitSettingsSection";
import { CandidateCard } from "../parts/CandidateCard";
import { RecentRepositoriesList } from "../parts/RecentRepositoriesList";

const PROBE_DEBOUNCE_MS = 300;

/** Verdicts that make it unsafe to register this folder as-is. */
function isCandidateBlocking(candidate: ProjectCandidate | null): boolean {
  if (!candidate) return false;
  switch (candidate.kind) {
    case "notFound":
    case "notADirectory":
    case "detachedHead":
    case "nestedInRepository":
    case "nonEmptyNonRepo":
      return true;
    case "repository":
      return candidate.alreadyRegisteredAs !== null;
    default:
      return false;
  }
}

export interface ExistingRepositoryStepProps {
  onCreate: (project: CreateProject) => void | Promise<void>;
  onBrowseFolder?: ((options?: { title?: string }) => Promise<string | null>) | undefined;
  onInspectCandidate?: ((path: string) => Promise<ProjectCandidate>) | undefined;
  onClose: () => void;
  onSwitchToCreateNew: (path: string) => void;
  isCreating: boolean;
  isFirstRun?: boolean | undefined;
  error?: string | null | undefined;
  initialWorkingDirectory?: string | undefined;
}

export function ExistingRepositoryStep({
  onCreate,
  onBrowseFolder,
  onInspectCandidate,
  onClose,
  onSwitchToCreateNew,
  isCreating,
  isFirstRun = false,
  error = null,
  initialWorkingDirectory = "",
}: ExistingRepositoryStepProps) {
  const [form, setForm] = useState<FormState>({
    name: "",
    workingDirectory: initialWorkingDirectory,
    baseBranch: "",
    worktreeParentDirectory: "",
  });
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const [submitted, setSubmitted] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [isNameManuallySet, setIsNameManuallySet] = useState(false);
  const [lastInferredName, setLastInferredName] = useState("");

  const [candidate, setCandidate] = useState<ProjectCandidate | null>(null);
  const [probing, setProbing] = useState(false);
  const probeGeneration = useRef(0);
  const [worktreeParentBlocking, setWorktreeParentBlocking] = useState(false);

  const recentRepositories = useUiStore((state) => state.recentRepositories);
  const recordRecentRepository = useUiStore((state) => state.recordRecentRepository);

  const errors = useMemo(() => validateForm(form), [form]);
  const hasErrors =
    Object.keys(errors).length > 0 || isCandidateBlocking(candidate) || worktreeParentBlocking;

  const worktreePath = useMemo(
    () => generateWorktreePath(form.workingDirectory, form.worktreeParentDirectory),
    [form.workingDirectory, form.worktreeParentDirectory]
  );

  // Debounced probe: fires after a paint boundary so folder-selection stays
  // click-synchronous. A generation counter rejects superseded responses.
  useEffect(() => {
    const path = form.workingDirectory;
    if (!path) {
      setCandidate(null);
      setProbing(false);
      return;
    }
    if (!onInspectCandidate) return;

    const generation = ++probeGeneration.current;
    setProbing(true);
    const timer = setTimeout(() => {
      onInspectCandidate(path)
        .then((result) => {
          if (probeGeneration.current !== generation) return;
          setCandidate(result);
          if (result.kind === "repository") {
            const preferred =
              (result.defaultBranch && result.branches.includes(result.defaultBranch)
                ? result.defaultBranch
                : result.branches.find((b) => b === "main" || b === "master")) ??
              result.branches[0];
            if (preferred) {
              setForm((prev) => ({ ...prev, baseBranch: preferred }));
            }
          }
        })
        .catch(() => {
          if (probeGeneration.current !== generation) return;
          setCandidate({ kind: "inspectionFailed", message: "Inspection failed" });
        })
        .finally(() => {
          if (probeGeneration.current === generation) setProbing(false);
        });
    }, PROBE_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [form.workingDirectory, onInspectCandidate]);

  const applyWorkingDirectory = useCallback(
    (path: string) => {
      const inferredName = extractFolderName(path);
      setForm((prev) => {
        const shouldAutoFill = !isNameManuallySet || prev.name === lastInferredName || prev.name === "";
        return {
          ...prev,
          workingDirectory: path,
          name: shouldAutoFill ? inferredName : prev.name,
        };
      });
      setLastInferredName(inferredName);
      setTouched((prev) => ({ ...prev, workingDirectory: true }));
    },
    [isNameManuallySet, lastInferredName]
  );

  const handleBrowse = useCallback(async () => {
    if (!onBrowseFolder) return;
    const path = await onBrowseFolder({ title: "Select Project Folder" });
    if (path) applyWorkingDirectory(path);
  }, [onBrowseFolder, applyWorkingDirectory]);

  const handleBrowseWorktreeParent = useCallback(async () => {
    if (!onBrowseFolder) return;
    const path = await onBrowseFolder({ title: "Select Worktree Parent Folder" });
    if (path) setForm((prev) => ({ ...prev, worktreeParentDirectory: path }));
  }, [onBrowseFolder]);

  const handleNameChange = useCallback(
    (value: string) => {
      setForm((prev) => ({ ...prev, name: value }));
      if (value !== lastInferredName) setIsNameManuallySet(true);
    },
    [lastInferredName]
  );

  const handleSubmit = useCallback(() => {
    setSubmitted(true);
    setTouched({ name: true, workingDirectory: true, baseBranch: true });

    const currentErrors = validateForm(form);
    if (
      Object.keys(currentErrors).length > 0 ||
      isCandidateBlocking(candidate) ||
      worktreeParentBlocking
    )
      return;

    const projectName = form.name.trim() || extractFolderName(form.workingDirectory);
    const workingDirectory = form.workingDirectory.trim();
    void (async () => {
      try {
        await onCreate({
          name: projectName,
          workingDirectory,
          gitMode: "worktree",
          baseBranch: form.baseBranch.trim(),
          worktreeParentDirectory: form.worktreeParentDirectory.trim() || "~/ralphx-worktrees",
        });
        recordRecentRepository(workingDirectory, projectName);
      } catch {
        // Creation failed; the wizard's error prop surfaces the failure.
      }
    })();
  }, [form, candidate, worktreeParentBlocking, onCreate, recordRecentRepository]);

  return (
    <>
      <div className="px-6 py-5 space-y-5">
        <div className="space-y-1.5">
          <Label
            htmlFor="folder-input"
            className="text-sm font-medium text-[var(--text-secondary)]"
          >
            Location <span className="text-[var(--status-error)]">*</span>
          </Label>
          <div className="flex gap-2">
            <Input
              id="folder-input"
              data-testid="folder-input"
              type="text"
              value={form.workingDirectory}
              onChange={(e) => applyWorkingDirectory(e.target.value)}
              placeholder="Select a folder..."
              readOnly
              disabled={isCreating}
              className={cn(
                "flex-1 h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
                (touched.workingDirectory || submitted) && errors.workingDirectory
                  ? "border-[var(--status-error)]"
                  : "border-[var(--border-subtle)]",
                isCreating && "opacity-50"
              )}
            />
            {onBrowseFolder && (
              <Button
                data-testid="browse-button"
                type="button"
                onClick={handleBrowse}
                disabled={isCreating}
                variant="secondary"
                className="h-10 px-3 gap-2 bg-[var(--bg-elevated)] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] border-0"
              >
                <FolderOpen className="h-4 w-4" />
                Browse
              </Button>
            )}
          </div>
          {(touched.workingDirectory || submitted) && errors.workingDirectory && (
            <p data-testid="folder-input-error" className="text-xs text-[var(--status-error)]">
              {errors.workingDirectory}
            </p>
          )}
        </div>

        {!form.workingDirectory && (
          <RecentRepositoriesList
            recents={recentRepositories}
            onSelect={applyWorkingDirectory}
            disabled={isCreating}
          />
        )}

        {probing && (
          <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <Loader2 className="h-3 w-3 animate-spin" />
            Checking this folder...
          </div>
        )}
        {!probing && candidate && (
          <CandidateCard
            candidate={candidate}
            probedPath={form.workingDirectory}
            onSwitchToCreateNew={onSwitchToCreateNew}
            onUseRepositoryRoot={(root) => applyWorkingDirectory(root)}
            onOpenExisting={onClose}
          />
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
            value={form.name}
            onChange={(e) => handleNameChange(e.target.value)}
            placeholder={
              form.workingDirectory ? extractFolderName(form.workingDirectory) || "my-app" : "Auto-inferred from folder"
            }
            disabled={isCreating}
            className={cn(
              "h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]",
              isCreating && "opacity-50"
            )}
          />
        </div>

        <Separator className="bg-[var(--border-subtle)]" />

        <GitSettingsSection
          baseBranchMode="select"
          baseBranchLabel="Base branch"
          baseBranch={form.baseBranch}
          onBaseBranchChange={(value) => setForm((prev) => ({ ...prev, baseBranch: value }))}
          branches={candidate?.kind === "repository" ? candidate.branches : []}
          loadingBranches={probing}
          baseBranchError={errors.baseBranch}
          baseBranchTouched={touched.baseBranch || submitted}
          worktreePath={worktreePath}
          worktreeParentDirectory={form.worktreeParentDirectory}
          onWorktreeParentDirectoryChange={(value) =>
            setForm((prev) => ({ ...prev, worktreeParentDirectory: value }))
          }
          worktreeParentRepositoryRoot={
            candidate?.kind === "repository" ? candidate.repositoryRoot : undefined
          }
          onBrowseWorktreeParent={onBrowseFolder ? handleBrowseWorktreeParent : undefined}
          onWorktreeParentBlockingChange={setWorktreeParentBlocking}
          showAdvanced={showAdvanced}
          onShowAdvancedChange={setShowAdvanced}
          isCreating={isCreating}
        />

        {error && (
          <div
            data-testid="wizard-error"
            className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--status-error-muted)] text-[var(--status-error)]"
          >
            <AlertTriangle className="h-3.5 w-3.5" />
            <span className="text-sm">{error}</span>
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
