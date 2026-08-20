/**
 * CloneStep - "Clone Repository" intent.
 *
 * Three internal phases: "configure" (URL + destination, debounced
 * validate_clone_target), "running" (useCloneJob drives progress), and
 * "settings" (prefilled from the completed clone, finishes through the
 * unchanged onCreate/create_project path shared by every other step).
 *
 * Every on-screen failure is mapped from the backend's typed failure code to
 * a plain-language sentence - no git commands, ref syntax, or raw codes are
 * ever rendered (explicit product constraint).
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { CreateProject } from "@/types/project";
import { projectsApi, type CloneTargetPlan, type StartProjectCloneInput } from "@/api/projects";
import { DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useUiStore } from "@/stores/uiStore";
import { useCloneJob } from "@/hooks/useCloneJob";
import { extractFolderName, generateWorktreePath } from "../ProjectCreationWizard.helpers";
import { GitSettingsSection } from "../parts/GitSettingsSection";
import { CloneProgress } from "../parts/CloneProgress";
import { CloneConfigureForm } from "../parts/CloneConfigureForm";
import { ErrorBanner } from "../parts/CloneErrorBanner";

const VALIDATE_DEBOUNCE_MS = 300;

type CloneStepPhase = "configure" | "running" | "settings";

export interface CloneStepProps {
  onCreate: (project: CreateProject) => Promise<void> | void;
  onBrowseFolder?: ((options?: { title?: string }) => Promise<string | null>) | undefined;
  onClose: () => void;
  isCreating: boolean;
  isFirstRun?: boolean | undefined;
  error?: string | null | undefined;
  /** Reports whether a clone is actively running, so the wizard shell can block Escape/backdrop dismissal. */
  onActiveChange?: ((active: boolean) => void) | undefined;
}

export function CloneStep({
  onCreate,
  onBrowseFolder,
  onClose,
  isCreating,
  isFirstRun = false,
  error = null,
  onActiveChange,
}: CloneStepProps) {
  const job = useCloneJob();

  const [url, setUrl] = useState("");
  const [parentDirectory, setParentDirectory] = useState("");
  const [folderName, setFolderName] = useState("");
  const [isFolderNameManuallySet, setIsFolderNameManuallySet] = useState(false);
  const [plan, setPlan] = useState<CloneTargetPlan | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [isCancelling, setIsCancelling] = useState(false);
  const [phase, setPhase] = useState<CloneStepPhase>("configure");
  const generationRef = useRef(0);

  const [settingsName, setSettingsName] = useState("");
  const [settingsBaseBranch, setSettingsBaseBranch] = useState("");
  const [settingsWorktreeParent, setSettingsWorktreeParent] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [worktreeParentBlocking, setWorktreeParentBlocking] = useState(false);

  const [depth, setDepth] = useState("");
  const [singleBranch, setSingleBranch] = useState(false);
  const [recurseSubmodules, setRecurseSubmodules] = useState(false);
  const [showAdvancedCloneOptions, setShowAdvancedCloneOptions] = useState(false);

  const recordRecentRepository = useUiStore((state) => state.recordRecentRepository);

  const settingsWorktreePath = generateWorktreePath(settingsName, settingsWorktreeParent);

  // Report cloning-active state so the wizard shell can block Escape/backdrop
  // dismissal; Cancel Clone is the only exit while a clone is running.
  useEffect(() => {
    onActiveChange?.(phase === "running");
    return () => onActiveChange?.(false);
  }, [phase, onActiveChange]);

  // Debounced validate_clone_target with a generation counter: any response
  // whose generation is no longer current is discarded.
  useEffect(() => {
    if (!url.trim()) {
      setPlan(null);
      return;
    }
    const generation = ++generationRef.current;
    const timer = setTimeout(() => {
      projectsApi
        .validateCloneTarget({
          url: url.trim(),
          ...(parentDirectory.trim() && { parentDirectory: parentDirectory.trim() }),
          ...(folderName.trim() && { folderName: folderName.trim() }),
        })
        .then((result) => {
          if (generationRef.current !== generation) return;
          setPlan(result);
          if (!isFolderNameManuallySet && result.folderName) {
            setFolderName(result.folderName);
          }
        })
        .catch(() => {
          if (generationRef.current !== generation) return;
          setPlan({
            normalizedUrl: null,
            folderName: null,
            branch: null,
            suggestedSshUrl: null,
            destination: null,
            ready: false,
            problem: "Could not check this repository. Try again.",
          });
        });
    }, VALIDATE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [url, parentDirectory, folderName, isFolderNameManuallySet]);

  // On terminal state: completed advances to the prefilled settings phase;
  // any other terminal outcome returns to configure so the user can retry.
  useEffect(() => {
    if (phase !== "running" || !job.status) return;
    if (job.status.state === "completed") {
      const destination = job.status.destination;
      setSettingsName(extractFolderName(destination));
      setSettingsBaseBranch(job.status.defaultBranch ?? "main");
      setPhase("settings");
      return;
    }
    if (job.status.state === "running") return;
    setPhase("configure");
  }, [job.status, phase]);

  const handleFolderNameChange = useCallback((value: string) => {
    setIsFolderNameManuallySet(true);
    setFolderName(value);
  }, []);

  const handleBrowseParent = useCallback(async () => {
    if (!onBrowseFolder) return;
    const path = await onBrowseFolder({ title: "Select Parent Folder" });
    if (path) setParentDirectory(path);
  }, [onBrowseFolder]);

  const handleBrowseWorktreeParent = useCallback(async () => {
    if (!onBrowseFolder) return;
    const path = await onBrowseFolder({ title: "Select Worktree Parent Folder" });
    if (path) setSettingsWorktreeParent(path);
  }, [onBrowseFolder]);

  const handleUseSshUrl = useCallback(() => {
    if (!plan?.suggestedSshUrl) return;
    setIsFolderNameManuallySet(false);
    setUrl(plan.suggestedSshUrl);
  }, [plan]);

  const handleSelectRepo = useCallback((nameWithOwner: string) => {
    setIsFolderNameManuallySet(false);
    setUrl(nameWithOwner);
  }, []);

  const handleStart = useCallback(async () => {
    setSubmitted(true);
    if (!plan || !plan.ready || !parentDirectory.trim()) return;
    const parsedDepth = Number.parseInt(depth, 10);
    const input: StartProjectCloneInput = {
      url: (plan.normalizedUrl ?? url).trim(),
      parentDirectory: parentDirectory.trim(),
      ...(folderName.trim() && { folderName: folderName.trim() }),
      ...(plan.branch && { branch: plan.branch }),
      ...(Number.isFinite(parsedDepth) && parsedDepth > 0 && { depth: parsedDepth }),
      ...(singleBranch && { singleBranch: true }),
      ...(recurseSubmodules && { recurseSubmodules: true }),
    };
    setPhase("running");
    const started = await job.start(input);
    if (!started) setPhase("configure");
  }, [plan, parentDirectory, folderName, url, depth, singleBranch, recurseSubmodules, job]);

  const handleCancel = useCallback(async () => {
    setIsCancelling(true);
    try {
      await job.cancel();
    } finally {
      setIsCancelling(false);
    }
  }, [job]);

  const handleCreateFromSettings = useCallback(() => {
    if (job.status?.state !== "completed" || worktreeParentBlocking) return;
    const destination = job.status.destination;
    const projectName = settingsName.trim() || extractFolderName(destination);
    void (async () => {
      try {
        await onCreate({
          name: projectName,
          workingDirectory: destination,
          gitMode: "worktree",
          baseBranch: settingsBaseBranch.trim() || "main",
          worktreeParentDirectory: settingsWorktreeParent.trim() || "~/ralphx-worktrees",
        });
        recordRecentRepository(destination, projectName);
      } catch {
        // Creation failed; the wizard's error prop surfaces the failure.
      }
    })();
  }, [
    job.status,
    worktreeParentBlocking,
    settingsName,
    settingsBaseBranch,
    settingsWorktreeParent,
    onCreate,
    recordRecentRepository,
  ]);

  const primaryDisabled = !plan?.ready || !parentDirectory.trim() || isCreating;

  if (phase === "running") {
    return (
      <div className="px-6 py-5">
        <CloneProgress
          phase={job.phase}
          percent={job.percent}
          received={job.received}
          total={job.total}
          lines={job.lines}
          onCancel={handleCancel}
          isCancelling={isCancelling}
        />
      </div>
    );
  }

  if (phase === "settings") {
    return (
      <>
        <div className="px-6 py-5 space-y-5">
          <div
            data-testid="clone-settings-destination"
            className="px-3 py-2 rounded-lg bg-[var(--bg-base)] text-sm truncate text-[var(--text-secondary)]"
          >
            {job.status?.state === "completed" ? job.status.destination : ""}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="project-name-input" className="text-sm font-medium text-[var(--text-secondary)]">
              Project Name <span className="text-[var(--text-muted)]">(optional)</span>
            </Label>
            <Input
              id="project-name-input"
              data-testid="project-name-input"
              type="text"
              value={settingsName}
              onChange={(e) => setSettingsName(e.target.value)}
              disabled={isCreating}
              className="h-10 px-3 py-2 rounded-lg text-sm bg-[var(--bg-base)] border border-[var(--border-subtle)] text-[var(--text-primary)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:border-[var(--accent-primary)]"
            />
          </div>

          <Separator className="bg-[var(--border-subtle)]" />

          <GitSettingsSection
            baseBranchMode="freeText"
            baseBranchLabel="Default branch"
            baseBranch={settingsBaseBranch}
            onBaseBranchChange={setSettingsBaseBranch}
            worktreePath={settingsWorktreePath}
            worktreeParentDirectory={settingsWorktreeParent}
            onWorktreeParentDirectoryChange={setSettingsWorktreeParent}
            worktreeParentRepositoryRoot={
              job.status?.state === "completed" ? job.status.destination : undefined
            }
            onBrowseWorktreeParent={onBrowseFolder ? handleBrowseWorktreeParent : undefined}
            onWorktreeParentBlockingChange={setWorktreeParentBlocking}
            showAdvanced={showAdvanced}
            onShowAdvancedChange={setShowAdvanced}
            isCreating={isCreating}
          />

          {error && <ErrorBanner testId="wizard-error" text={error} />}
        </div>

        <DialogFooter className="px-6 py-4 border-t border-[var(--border-subtle)] gap-3 sm:gap-3">
          <Button
            data-testid="create-button"
            type="button"
            onClick={handleCreateFromSettings}
            disabled={isCreating || worktreeParentBlocking}
            className={cn(
              "gap-2",
              isCreating || worktreeParentBlocking
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

  return (
    <CloneConfigureForm
      url={url}
      onUrlChange={setUrl}
      onSelectRepo={handleSelectRepo}
      submitted={submitted}
      plan={plan}
      parentDirectory={parentDirectory}
      onParentDirectoryChange={setParentDirectory}
      folderName={folderName}
      onFolderNameChange={handleFolderNameChange}
      onBrowseParent={onBrowseFolder ? handleBrowseParent : undefined}
      depth={depth}
      onDepthChange={setDepth}
      singleBranch={singleBranch}
      onSingleBranchChange={setSingleBranch}
      recurseSubmodules={recurseSubmodules}
      onRecurseSubmodulesChange={setRecurseSubmodules}
      showAdvancedCloneOptions={showAdvancedCloneOptions}
      onShowAdvancedCloneOptionsChange={setShowAdvancedCloneOptions}
      jobStatus={job.status}
      jobError={job.error}
      onUseSshUrl={handleUseSshUrl}
      onRetry={handleStart}
      isCreating={isCreating}
      isFirstRun={isFirstRun}
      onClose={onClose}
      onStart={handleStart}
      primaryDisabled={primaryDisabled}
      error={error}
    />
  );
}
