import { useState, useCallback, useEffect } from "react";
import { GitBranch, Loader2, RefreshCw, CheckCircle2, XCircle } from "lucide-react";
import { toast } from "sonner";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { api, getGitDefaultBranch } from "@/lib/tauri";
import { GitAuthRepairPanel } from "@/components/git/GitAuthRepairPanel";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";
import type { MergeValidationMode } from "@/types/project";
import {
  SectionCard,
  SelectSettingRow,
  SettingRow,
  ToggleSettingRow,
} from "./SettingsView.shared";
import {
  useGitRemoteUrl,
  useGhAuthStatus,
  useUpdateGithubPrEnabled,
} from "@/hooks/useGithubSettings";

const VALIDATION_MODE_OPTIONS: {
  value: MergeValidationMode;
  label: string;
  description: string;
}[] = [
  {
    value: "block",
    label: "Block on Failure",
    description: "Validation failure pauses merge — you decide",
  },
  {
    value: "auto_fix",
    label: "Auto-fix",
    description: "AI agent attempts to fix validation errors before asking you",
  },
  {
    value: "warn",
    label: "Warn on Failure",
    description: "Merge continues, validation issues logged as warnings",
  },
  {
    value: "off",
    label: "Disabled",
    description: "Skip merge validation entirely",
  },
];

function SubsectionLabel({
  children,
  hint,
}: {
  children: React.ReactNode;
  hint?: string;
}) {
  return (
    <div className="settings-section__head">
      <span className="settings-section__label">
        {children}
      </span>
      {hint && (
        <span className="settings-section__chip">
          {hint}
        </span>
      )}
    </div>
  );
}

function isGithubRemoteUrl(remoteUrl: string | null | undefined): boolean {
  if (!remoteUrl) return false;

  const trimmed = remoteUrl.trim();
  if (trimmed.startsWith("git@github.com:")) {
    return true;
  }

  try {
    const parsed = new URL(trimmed);
    return (
      parsed.hostname === "github.com" &&
      (parsed.protocol === "https:" || parsed.protocol === "ssh:")
    );
  } catch {
    return false;
  }
}

function TextSettingRow({
  id,
  label,
  description,
  value,
  placeholder,
  disabled,
  onChange,
  onBlur,
  actionLabel,
  actionIcon,
  onAction,
  actionLoading,
}: {
  id: string;
  label: string;
  description: string;
  value: string;
  placeholder?: string;
  disabled: boolean;
  onChange: (value: string) => void;
  onBlur?: () => void;
  actionLabel?: string;
  actionIcon?: React.ReactNode;
  onAction?: () => void;
  actionLoading?: boolean;
}) {
  return (
    <SettingRow id={id} label={label} description={description} isDisabled={disabled}>
      <div className="settings-row__control">
        <Input
          id={id}
          data-testid={id}
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
          className="settings-input w-[200px]"
        />
        {onAction && (
          <Button
            variant="ghost"
            size="sm"
            onClick={onAction}
            disabled={disabled || actionLoading}
            className="settings-btn-ghost"
            title={actionLabel}
          >
            {actionLoading ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              actionIcon
            )}
            {actionLabel && <span className="ml-1">{actionLabel}</span>}
          </Button>
        )}
      </div>
    </SettingRow>
  );
}

export function RepositorySettingsSection() {
  const project = useProjectStore(selectActiveProject);
  const updateProject = useProjectStore((s) => s.updateProject);

  const [isUpdating, setIsUpdating] = useState(false);
  const [isDetectingDefault, setIsDetectingDefault] = useState(false);
  const [pendingBaseBranch, setPendingBaseBranch] = useState<string | null>(null);
  const [pendingWorktreeDir, setPendingWorktreeDir] = useState<string | null>(null);

  const { data: remoteUrl, isLoading: isLoadingRemote } = useGitRemoteUrl(
    project?.id ?? null
  );
  const { data: isGhAuthed, isLoading: isLoadingAuth } = useGhAuthStatus();
  const updatePrEnabled = useUpdateGithubPrEnabled();

  useEffect(() => {
    setPendingBaseBranch(null);
    setPendingWorktreeDir(null);
  }, [project?.id]);

  const handleBaseBranchChange = useCallback((value: string) => {
    setPendingBaseBranch(value);
  }, []);

  const handleBaseBranchBlur = useCallback(async () => {
    if (!project || pendingBaseBranch === null) return;
    const newValue = pendingBaseBranch.trim();
    if (newValue === (project.baseBranch ?? "")) {
      setPendingBaseBranch(null);
      return;
    }
    setIsUpdating(true);
    try {
      await api.projects.update(project.id, { baseBranch: newValue || null });
      updateProject(project.id, { baseBranch: newValue || null });
      setPendingBaseBranch(null);
      toast.success("Base branch updated");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to update base branch");
    } finally {
      setIsUpdating(false);
    }
  }, [project, pendingBaseBranch, updateProject]);

  const handleDetectDefaultBranch = useCallback(async () => {
    if (!project?.workingDirectory) {
      toast.error("No working directory set for this project");
      return;
    }
    setIsDetectingDefault(true);
    try {
      const defaultBranch = await getGitDefaultBranch(project.workingDirectory);
      setIsUpdating(true);
      await api.projects.update(project.id, { baseBranch: defaultBranch });
      updateProject(project.id, { baseBranch: defaultBranch });
      setPendingBaseBranch(null);
      toast.success(`Detected default branch: ${defaultBranch}`);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to detect default branch");
    } finally {
      setIsDetectingDefault(false);
      setIsUpdating(false);
    }
  }, [project, updateProject]);

  const handleWorktreeDirChange = useCallback((value: string) => {
    setPendingWorktreeDir(value);
  }, []);

  const handleWorktreeDirBlur = useCallback(async () => {
    if (!project || pendingWorktreeDir === null) return;
    const newValue = pendingWorktreeDir.trim();
    if (newValue === (project.worktreeParentDirectory ?? "~/ralphx-worktrees")) {
      setPendingWorktreeDir(null);
      return;
    }
    setIsUpdating(true);
    try {
      await api.projects.update(project.id, {
        worktreeParentDirectory: newValue || null,
      });
      updateProject(project.id, { worktreeParentDirectory: newValue || null });
      setPendingWorktreeDir(null);
      toast.success("Worktree location updated");
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to update worktree location");
    } finally {
      setIsUpdating(false);
    }
  }, [project, pendingWorktreeDir, updateProject]);

  const handleValidationModeChange = useCallback(
    async (newMode: MergeValidationMode) => {
      if (!project || newMode === project.mergeValidationMode) return;
      setIsUpdating(true);
      try {
        await api.projects.update(project.id, { mergeValidationMode: newMode });
        updateProject(project.id, { mergeValidationMode: newMode });
        const labels: Record<MergeValidationMode, string> = {
          block: "Block on Failure",
          auto_fix: "Auto-fix",
          warn: "Warn on Failure",
          off: "Disabled",
        };
        toast.success(`Merge validation set to ${labels[newMode]}`);
      } catch (error) {
        toast.error(
          error instanceof Error ? error.message : "Failed to update merge validation mode"
        );
      } finally {
        setIsUpdating(false);
      }
    },
    [project, updateProject]
  );

  const handlePrToggle = async () => {
    if (!project) return;
    try {
      await updatePrEnabled.mutateAsync({
        projectId: project.id,
        enabled: !project.githubPrEnabled,
      });
      toast.success(project.githubPrEnabled ? "PR mode disabled" : "PR mode enabled");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to update PR mode");
    }
  };

  if (!project) return null;

  const baseBranch = pendingBaseBranch ?? project.baseBranch ?? "";
  const worktreeParentDirectory =
    pendingWorktreeDir ?? project.worktreeParentDirectory ?? "~/ralphx-worktrees";

  const isGithubRemote = isGithubRemoteUrl(remoteUrl);
  const isToggleDisabled = !isGithubRemote || updatePrEnabled.isPending;
  const isSaving = isUpdating || updatePrEnabled.isPending;

  return (
    <SectionCard
      icon={<GitBranch className="w-[18px] h-[18px] text-[var(--card-icon-color)]" />}
      title="Repository"
      description="Version control and GitHub integration"
    >
      <SubsectionLabel>Branching</SubsectionLabel>
      <TextSettingRow
        id="base-branch"
        label="Base Branch"
        description="The branch tasks are merged into"
        value={baseBranch}
        placeholder="main"
        disabled={isUpdating || isDetectingDefault}
        onChange={handleBaseBranchChange}
        onBlur={handleBaseBranchBlur}
        actionLabel="Detect"
        actionIcon={<RefreshCw className="w-3.5 h-3.5" />}
        onAction={handleDetectDefaultBranch}
        actionLoading={isDetectingDefault}
      />
      <TextSettingRow
        id="worktree-location"
        label="Worktree Location"
        description="Directory where task worktrees are created"
        value={worktreeParentDirectory}
        placeholder="~/ralphx-worktrees"
        disabled={isUpdating}
        onChange={handleWorktreeDirChange}
        onBlur={handleWorktreeDirBlur}
      />

      <SubsectionLabel>Merge Behavior</SubsectionLabel>
      <SelectSettingRow
        id="merge-validation-mode"
        label="Merge Validation"
        description="Run build checks after merging task branches"
        value={project.mergeValidationMode}
        options={VALIDATION_MODE_OPTIONS}
        disabled={isUpdating}
        onChange={handleValidationModeChange}
      />
      <ToggleSettingRow
        id="github-pr-enabled"
        label="GitHub PR Mode"
        description={
          !isGithubRemote
            ? "Remote is not GitHub — PR mode unavailable"
            : !isGhAuthed
            ? "Enable to create draft PRs when plans execute (gh auth required for PR operations)"
            : "Create draft PRs when plans execute instead of merging directly"
        }
        checked={isGithubRemote && (project.githubPrEnabled ?? false)}
        disabled={isToggleDisabled}
        onChange={handlePrToggle}
      />

      <SubsectionLabel hint="read-only">Diagnostics</SubsectionLabel>
      <div className="settings-diagnostics">
        <SettingRow
          id="github-remote-url"
          label="Remote URL"
          description="Git remote origin for this project"
        >
          {isLoadingRemote ? (
            <Loader2 className="w-4 h-4 animate-spin text-[var(--text-muted)]" />
          ) : (
            <span className="settings-readonly-value max-w-[240px] truncate">
              {remoteUrl ?? "Not configured"}
            </span>
          )}
        </SettingRow>
        <SettingRow
          id="gh-auth-status"
          label="GitHub CLI"
          description="gh auth status — required for PR operations"
        >
          {isLoadingAuth ? (
            <Loader2 className="w-4 h-4 animate-spin text-[var(--text-muted)]" />
          ) : isGhAuthed ? (
            <div className="settings-status-badge settings-status-badge--ok">
              <CheckCircle2 className="w-3.5 h-3.5" />
              <span>Authenticated</span>
            </div>
          ) : (
            <div className="settings-status-badge">
              <XCircle className="w-3.5 h-3.5" />
              <span>Not authenticated</span>
            </div>
          )}
        </SettingRow>
        <div className="settings-diagnostics__repair">
          <GitAuthRepairPanel
            projectId={project.id}
            requiresGhAuth={project.githubPrEnabled ?? false}
          />
        </div>
      </div>

      {isSaving && (
        <div className="flex items-center gap-2 mt-2 text-xs text-[var(--text-muted)]">
          <Loader2 className="w-3 h-3 animate-spin" />
          <span>Saving...</span>
        </div>
      )}
    </SectionCard>
  );
}
