import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  Check,
  ClipboardList,
  FileDown,
  Info,
  Pencil,
  Pin,
  PinOff,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  X,
} from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import {
  PROJECT_SKILL_CATEGORY_VALUES,
  projectSkillsApi,
  type DistillProjectSkillsResult,
  type ProjectSkillCategory,
  type ProjectSkillImportCandidate,
  type ProjectSkillImportPreviewResult,
  type ProjectSkill,
  type ProjectSkillExportResult,
  type ProjectSkillPullRequestCandidate,
  type ProjectSkillReportCard,
  type UpdateProjectSkillInput,
} from "@/api/project-skills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

import {
  ProjectSkillsExportControls,
  ProjectSkillsExportSummary,
} from "./ProjectSkillsExportControls";

interface ProjectSkillsCuratorPanelProps {
  projectId: string;
  className?: string;
}

const stagedSkillsKey = (projectId: string) =>
  ["project-skills", projectId, "staged"] as const;

const approvedSkillsKey = (projectId: string) =>
  ["project-skills", projectId, "approved"] as const;

const settingsKey = (projectId: string) =>
  ["project-skills", projectId, "settings"] as const;

const reportCardsKey = (projectId: string) =>
  ["project-skills", projectId, "report-cards"] as const;

const PROJECT_SKILL_SOURCE_ROOT_OPTIONS = [
  { id: ".claude/skills", label: "Claude skills" },
  { id: ".codex/skills", label: "Codex skills" },
  { id: ".agents/skills", label: "Agent skills" },
  { id: ".ralphx/skills", label: "RalphX skills" },
] as const;

const PROJECT_SKILL_CATEGORY_OPTIONS: Array<{
  value: ProjectSkillCategory;
  label: string;
}> = PROJECT_SKILL_CATEGORY_VALUES.map((value) => ({
  value,
  label: value.charAt(0).toUpperCase() + value.slice(1),
}));

interface MemoryPromotionFormState {
  memoryId: string;
  title: string;
  bucket: ProjectSkillCategory;
  stage: ProjectSkillCategory;
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string;
}

const DEFAULT_MEMORY_PROMOTION: MemoryPromotionFormState = {
  memoryId: "",
  title: "",
  bucket: "review",
  stage: "review",
  compactGuidance: "",
  bodyMarkdown: "",
  predictedEffect: "",
};

type CandidateSourceMode = "stored" | "prs";

export function ProjectSkillsCuratorPanel({
  projectId,
  className,
}: ProjectSkillsCuratorPanelProps) {
  const queryClient = useQueryClient();
  const [exportPreview, setExportPreview] =
    useState<ProjectSkillExportResult | null>(null);
  const [candidateDialogOpen, setCandidateDialogOpen] = useState(false);
  const [candidateSourceMode, setCandidateSourceMode] =
    useState<CandidateSourceMode>("stored");
  const [candidateResult, setCandidateResult] =
    useState<DistillProjectSkillsResult | null>(null);
  const [pullRequestCandidates, setPullRequestCandidates] = useState<
    ProjectSkillPullRequestCandidate[]
  >([]);
  const [pullRequestStageResult, setPullRequestStageResult] = useState<{
    number: number;
    skippedExisting: boolean;
  } | null>(null);
  const [importManifest, setImportManifest] = useState("");
  const [importPreview, setImportPreview] =
    useState<ProjectSkillImportPreviewResult | null>(null);
  const [projectDirectoryImport, setProjectDirectoryImport] =
    useState<ProjectSkillImportPreviewResult | null>(null);
  const [projectDirectorySyncedCount, setProjectDirectorySyncedCount] =
    useState(0);
  const [sourceImportRoots, setSourceImportRoots] = useState<string[]>([
    ".claude/skills",
  ]);
  const [sourceImportSyncEnabled, setSourceImportSyncEnabled] = useState(false);
  const [memoryPromotion, setMemoryPromotion] = useState<MemoryPromotionFormState>(
    DEFAULT_MEMORY_PROMOTION,
  );
  const [localError, setLocalError] = useState<string | null>(null);
  const stagedQueryKey = stagedSkillsKey(projectId);
  const approvedQueryKey = approvedSkillsKey(projectId);
  const projectSkillSettingsKey = settingsKey(projectId);
  const projectSkillReportCardsKey = reportCardsKey(projectId);

  const stagedQuery = useQuery({
    queryKey: stagedQueryKey,
    queryFn: () =>
      projectSkillsApi.list({
        projectId,
        status: "staged",
        includeArchived: false,
      }),
  });

  const approvedQuery = useQuery({
    queryKey: approvedQueryKey,
    queryFn: () =>
      projectSkillsApi.list({
        projectId,
        status: "approved",
        includeArchived: false,
      }),
  });

  const settingsQuery = useQuery({
    queryKey: projectSkillSettingsKey,
    queryFn: () => projectSkillsApi.getSettings(projectId),
  });

  const reportCardsQuery = useQuery({
    queryKey: projectSkillReportCardsKey,
    queryFn: () =>
      projectSkillsApi.listReportCards({
        projectId,
        minLinkedOutcomes: 5,
        staleAfterDays: 30,
      }),
  });

  const invalidateSkills = () => {
    queryClient.invalidateQueries({ queryKey: stagedQueryKey });
    queryClient.invalidateQueries({ queryKey: approvedQueryKey });
    queryClient.invalidateQueries({ queryKey: projectSkillReportCardsKey });
  };

  const distillMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.distill({
        projectId,
        limit: 10,
        source: null,
        includeGitHistory: false,
        includeGithubPrHistory: false,
      }),
    onMutate: () => {
      setCandidateResult(null);
      setPullRequestStageResult(null);
    },
    onSuccess: (result) => {
      setCandidateResult(result);
      invalidateSkills();
    },
  });

  const listPullRequestsMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.listPullRequestCandidates({
        projectId,
        limit: 25,
      }),
    onMutate: () => {
      setCandidateResult(null);
      setPullRequestStageResult(null);
    },
    onSuccess: (result) => {
      setPullRequestCandidates(result.candidates);
    },
  });

  const stagePullRequestMutation = useMutation({
    mutationFn: (number: number) =>
      projectSkillsApi.stageFromPullRequest({
        projectId,
        number,
      }),
    onMutate: (number) => {
      setPullRequestStageResult({ number, skippedExisting: false });
    },
    onSuccess: (result, number) => {
      setPullRequestStageResult({ number, skippedExisting: result.skippedExisting });
      invalidateSkills();
    },
  });

  const approveMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.approve(skillId),
    onSuccess: invalidateSkills,
  });

  const rejectMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.reject(skillId),
    onSuccess: invalidateSkills,
  });

  const archiveMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.archive(skillId),
    onSuccess: invalidateSkills,
  });

  const pinMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.pin(skillId),
    onSuccess: invalidateSkills,
  });

  const unpinMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.unpin(skillId),
    onSuccess: invalidateSkills,
  });

  const updateSkillMutation = useMutation({
    mutationFn: (input: UpdateProjectSkillInput) => projectSkillsApi.update(input),
    onSuccess: invalidateSkills,
  });

  const updateSettingsMutation = useMutation({
    mutationFn: (exportEnabled: boolean) =>
      projectSkillsApi.updateSettings(projectId, { exportEnabled }),
    onSuccess: (settings) => {
      queryClient.setQueryData(projectSkillSettingsKey, settings);
    },
  });

  const previewExportMutation = useMutation({
    mutationFn: () => projectSkillsApi.previewExport(projectId),
    onSuccess: setExportPreview,
  });

  const applyExportMutation = useMutation({
    mutationFn: () => projectSkillsApi.applyExport(projectId),
    onSuccess: setExportPreview,
  });

  const previewImportMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.previewImport({
        projectId,
        candidates: parseImportManifest(importManifest),
      }),
    onMutate: () => setLocalError(null),
    onSuccess: setImportPreview,
    onError: (error) => setLocalError(error.message),
  });

  const applyImportMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.applyImport({
        projectId,
        confirmImport: true,
        candidates: parseImportManifest(importManifest),
      }),
    onMutate: () => setLocalError(null),
    onSuccess: (result) => {
      setImportPreview(result.preview);
      invalidateSkills();
    },
    onError: (error) => setLocalError(error.message),
  });

  const applyProjectDirectoryImportMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.applyProjectDirectoryImport({
        projectId,
        sourceRoots: sourceImportRoots,
        sourceSyncEnabled: sourceImportSyncEnabled,
      }),
    onSuccess: (result) => {
      setProjectDirectoryImport(result.preview);
      setProjectDirectorySyncedCount(result.syncedCount);
      invalidateSkills();
    },
  });

  const promoteMemoryMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.promoteMemory({
        projectId,
        memoryId: memoryPromotion.memoryId.trim(),
        title: memoryPromotion.title.trim() || null,
        bucket: memoryPromotion.bucket,
        stage: memoryPromotion.stage,
        compactGuidance: memoryPromotion.compactGuidance,
        bodyMarkdown: memoryPromotion.bodyMarkdown,
        predictedEffect: memoryPromotion.predictedEffect,
      }),
    onMutate: () => setLocalError(null),
    onSuccess: () => {
      invalidateSkills();
      setMemoryPromotion(DEFAULT_MEMORY_PROMOTION);
    },
    onError: (error) => setLocalError(error.message),
  });

  const stagedSkills = stagedQuery.data ?? [];
  const approvedSkills = approvedQuery.data ?? [];
  const reportCards = reportCardsQuery.data?.cards ?? [];
  const isBusy =
    distillMutation.isPending ||
    listPullRequestsMutation.isPending ||
    stagePullRequestMutation.isPending ||
    approveMutation.isPending ||
    rejectMutation.isPending ||
    archiveMutation.isPending ||
    pinMutation.isPending ||
    unpinMutation.isPending ||
    updateSkillMutation.isPending ||
    updateSettingsMutation.isPending ||
    previewExportMutation.isPending ||
    applyExportMutation.isPending ||
    previewImportMutation.isPending ||
    applyImportMutation.isPending ||
    applyProjectDirectoryImportMutation.isPending ||
    promoteMemoryMutation.isPending;
  const error =
    localError != null
      ? new Error(localError)
      : stagedQuery.error ??
        approvedQuery.error ??
        settingsQuery.error ??
        reportCardsQuery.error ??
        distillMutation.error ??
        listPullRequestsMutation.error ??
        stagePullRequestMutation.error ??
        approveMutation.error ??
        rejectMutation.error ??
        archiveMutation.error ??
        pinMutation.error ??
        unpinMutation.error ??
        updateSkillMutation.error ??
        updateSettingsMutation.error ??
        previewExportMutation.error ??
        applyExportMutation.error ??
        previewImportMutation.error ??
        applyImportMutation.error ??
        applyProjectDirectoryImportMutation.error ??
        promoteMemoryMutation.error;
  const exportEnabled = settingsQuery.data?.exportEnabled ?? false;
  const stagedCount = stagedSkills.length;
  const approvedCount = approvedSkills.length;
  const reportCardCount = reportCards.length;

  return (
    <section
      className={cn("flex min-h-0 flex-col gap-5", className)}
      aria-label="Learned skills"
    >
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <SkillMetricTile
          icon={ClipboardList}
          label="Review queue"
          value={stagedCount}
          detail={stagedCount === 1 ? "candidate waiting" : "candidates waiting"}
        />
        <SkillMetricTile
          icon={ShieldCheck}
          label="Approved"
          value={approvedCount}
          detail={approvedCount === 1 ? "skill available" : "skills available"}
        />
        <SkillMetricTile
          icon={Sparkles}
          label="Report cards"
          value={reportCardCount}
          detail="descriptive usage evidence"
        />
        <SkillMetricTile
          icon={FileDown}
          label="Export"
          value={exportEnabled ? "On" : "Off"}
          detail={exportEnabled ? "review branch required" : "explicit opt-in"}
        />
      </div>

      <div className="flex flex-wrap items-center justify-start gap-2 sm:justify-end">
        <CandidateDiscoveryDialog
          disabled={isBusy}
          open={candidateDialogOpen}
          pending={distillMutation.isPending}
          result={candidateResult}
          error={distillMutation.error}
          sourceMode={candidateSourceMode}
          onSourceModeChange={setCandidateSourceMode}
          onOpenChange={setCandidateDialogOpen}
          pullRequestCandidates={pullRequestCandidates}
          pullRequestPending={listPullRequestsMutation.isPending}
          pullRequestError={
            listPullRequestsMutation.error ?? stagePullRequestMutation.error
          }
          pullRequestStagePending={stagePullRequestMutation.isPending}
          pullRequestStageResult={pullRequestStageResult}
          onFindCandidates={() => distillMutation.mutate()}
          onLoadPullRequests={() => listPullRequestsMutation.mutate()}
          onStagePullRequest={(number) => stagePullRequestMutation.mutate(number)}
        />
      </div>

      <Tabs defaultValue="review" className="grid gap-4">
        <TabsList className="h-auto flex-wrap justify-start rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-1">
          <TabsTrigger value="review" className="text-xs">
            Review queue
          </TabsTrigger>
          <TabsTrigger value="approved" className="text-xs">
            Approved
          </TabsTrigger>
          <TabsTrigger value="reports" className="text-xs">
            Reports
          </TabsTrigger>
          <TabsTrigger value="advanced" className="text-xs">
            Advanced
          </TabsTrigger>
        </TabsList>

        <TabsContent value="review" className="mt-0 grid gap-4">
          {stagedQuery.isLoading ? (
            <Skeleton className="h-28 w-full" />
          ) : (
            <SkillSection
              title="Review Queue"
              description="Staged candidates must be approved before agents can use them."
              emptyMessage="No staged learned skills."
              skills={stagedSkills}
              renderSkill={(skill) => (
                <ProjectSkillCandidateCard
                  key={skill.id}
                  skill={skill}
                  disabled={isBusy}
                  onApprove={() => approveMutation.mutate(skill.id)}
                  onReject={() => rejectMutation.mutate(skill.id)}
                  onArchive={() => archiveMutation.mutate(skill.id)}
                  onUpdate={(input) => updateSkillMutation.mutate(input)}
                />
              )}
            />
          )}
        </TabsContent>

        <TabsContent value="approved" className="mt-0 grid gap-4">
          <div
            className="flex flex-wrap items-start justify-between gap-3 rounded-md border px-3 py-2 text-[0.6875rem] leading-relaxed"
            style={{
              backgroundColor: "var(--notice-info-bg)",
              borderColor: "var(--notice-info-border)",
              color: "var(--notice-info-text)",
            }}
          >
            <div className="flex min-w-0 flex-1 items-start gap-2">
              <Info
                className="mt-0.5 h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--notice-info-icon)" }}
              />
              <p>
                Approved skills are available to agents for this project.
                Export is optional and opens in a separate review dialog.
              </p>
            </div>
            <ProjectSkillsExportDialog
              disabled={isBusy}
              exportEnabled={exportEnabled}
              exportPreview={exportPreview}
              approvedCount={approvedSkills.length}
              onExportEnabledChange={(enabled) =>
                updateSettingsMutation.mutate(enabled)
              }
              onPreview={() => previewExportMutation.mutate()}
              onApply={() => applyExportMutation.mutate()}
            />
          </div>
          {approvedQuery.isLoading ? (
            <Skeleton className="h-28 w-full" />
          ) : (
            <SkillSection
              title="Approved Skills"
              description="Approved skills are eligible for future injection and export."
              emptyMessage="No approved learned skills."
              skills={approvedSkills}
              renderSkill={(skill) => (
                <ProjectSkillApprovedCard
                  key={skill.id}
                  skill={skill}
                  disabled={isBusy}
                  onPin={() => pinMutation.mutate(skill.id)}
                  onUnpin={() => unpinMutation.mutate(skill.id)}
                  onArchive={() => archiveMutation.mutate(skill.id)}
                  onUpdate={(input) => updateSkillMutation.mutate(input)}
                />
              )}
            />
          )}
        </TabsContent>

        <TabsContent value="reports" className="mt-0 grid gap-4">
          <ReportCardSection
            loading={reportCardsQuery.isLoading}
            cards={reportCards}
          />
        </TabsContent>

        <TabsContent value="advanced" className="mt-0 grid gap-4">
          <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs leading-5 text-[var(--text-secondary)]">
            Advanced intake is for controlled imports and one-off memory
            promotions. New rows still land in the Review Queue and require
            approval before agents can use them.
          </div>
          <ImportPromotionPanel
            disabled={isBusy}
            importManifest={importManifest}
            importPreview={importPreview}
            projectDirectoryImport={projectDirectoryImport}
            projectDirectorySyncedCount={projectDirectorySyncedCount}
            sourceImportRoots={sourceImportRoots}
            sourceImportSyncEnabled={sourceImportSyncEnabled}
            memoryPromotion={memoryPromotion}
            onImportManifestChange={setImportManifest}
            onPreviewImport={() => previewImportMutation.mutate()}
            onApplyImport={() => applyImportMutation.mutate()}
            onSourceImportRootsChange={setSourceImportRoots}
            onSourceImportSyncEnabledChange={setSourceImportSyncEnabled}
            onApplyProjectDirectoryImport={() =>
              applyProjectDirectoryImportMutation.mutate()
            }
            onMemoryPromotionChange={setMemoryPromotion}
            onPromoteMemory={() => promoteMemoryMutation.mutate()}
          />
        </TabsContent>
      </Tabs>

      {error ? (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error)]/30 bg-[var(--status-error)]/10 px-3 py-2 text-sm text-[var(--status-error)]"
        >
          {error.message}
        </div>
      ) : null}
    </section>
  );
}

function CandidateDiscoveryDialog({
  disabled,
  open,
  pending,
  result,
  error,
  sourceMode,
  onSourceModeChange,
  onOpenChange,
  pullRequestCandidates,
  pullRequestPending,
  pullRequestError,
  pullRequestStagePending,
  pullRequestStageResult,
  onFindCandidates,
  onLoadPullRequests,
  onStagePullRequest,
}: {
  disabled: boolean;
  open: boolean;
  pending: boolean;
  result: DistillProjectSkillsResult | null;
  error: Error | null;
  sourceMode: CandidateSourceMode;
  onSourceModeChange: (mode: CandidateSourceMode) => void;
  onOpenChange: (open: boolean) => void;
  pullRequestCandidates: ProjectSkillPullRequestCandidate[];
  pullRequestPending: boolean;
  pullRequestError: Error | null;
  pullRequestStagePending: boolean;
  pullRequestStageResult: { number: number; skippedExisting: boolean } | null;
  onFindCandidates: () => void;
  onLoadPullRequests: () => void;
  onStagePullRequest: (number: number) => void;
}) {
  const sourceDescription =
    sourceMode === "prs"
      ? "Loads recent GitHub pull request metadata only. Pick one PR to create a draft for review."
      : "Scans only RalphX-recorded task, conversation, PR, review, and workspace outcomes.";
  const actionPending = sourceMode === "prs" ? pullRequestPending : pending;
  const actionDisabled = disabled || actionPending;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button
          type="button"
          size="sm"
          disabled={disabled}
          className="w-full sm:w-auto"
        >
          <RefreshCw className={cn(actionPending && "animate-spin")} />
          Find candidates...
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-[560px]">
        <DialogHeader>
          <div>
            <DialogTitle>Find skill candidates</DialogTitle>
            <DialogDescription className="mt-1 leading-5">
              Scan stored task, conversation, and agent workspace outcomes for
              reusable procedures. Matching lessons are staged in the Review
              Queue; nothing is approved or injected automatically.
            </DialogDescription>
          </div>
        </DialogHeader>
        <div className="grid gap-4 px-6 py-5">
          <div className="grid gap-2">
            <div className="text-xs font-medium uppercase text-[var(--text-tertiary)]">
              Candidate source
            </div>
            <div className="flex flex-wrap gap-2">
              {[
                { id: "stored" as const, label: "Stored outcomes" },
                { id: "prs" as const, label: "GitHub PRs" },
              ].map((option) => (
                <Button
                  key={option.id}
                  type="button"
                  size="sm"
                  variant={sourceMode === option.id ? "default" : "outline"}
                  aria-pressed={sourceMode === option.id}
                  onClick={() => onSourceModeChange(option.id)}
                >
                  {option.label}
                </Button>
              ))}
            </div>
          </div>
          <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs leading-5 text-[var(--text-secondary)]">
            {sourceDescription} Duplicates already represented by an existing
            skill are skipped.
          </div>
          {pending && sourceMode === "stored" ? (
            <div className="flex items-center gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs text-[var(--text-secondary)]">
              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
              Finding reusable skill candidates...
            </div>
          ) : null}
          {pullRequestPending && sourceMode === "prs" ? (
            <div className="flex items-center gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs text-[var(--text-secondary)]">
              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
              Loading recent pull requests...
            </div>
          ) : null}
          {result && sourceMode === "stored" ? (
            <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs leading-5 text-[var(--text-secondary)]">
              Staged {result.stagedSkills.length} candidate
              {result.stagedSkills.length === 1 ? "" : "s"} in the Review
              Queue. Skipped {result.skippedExisting} duplicate
              {result.skippedExisting === 1 ? "" : "s"}.
            </div>
          ) : null}
          {sourceMode === "prs" && pullRequestCandidates.length > 0 ? (
            <div className="grid max-h-[280px] gap-2 overflow-y-auto rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] p-2">
              {pullRequestCandidates.map((pullRequest) => (
                <div
                  key={pullRequest.number}
                  className="grid gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-3 sm:grid-cols-[1fr_auto] sm:items-center"
                >
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium text-[var(--text-primary)]">
                      #{pullRequest.number} {pullRequest.title}
                    </div>
                    <div className="mt-1 flex flex-wrap gap-2 text-xs text-[var(--text-tertiary)]">
                      {pullRequest.state ? <span>{pullRequest.state}</span> : null}
                      {pullRequest.baseRefName ? (
                        <span>base {pullRequest.baseRefName}</span>
                      ) : null}
                      {pullRequest.updatedAt ? (
                        <span>updated {pullRequest.updatedAt.slice(0, 10)}</span>
                      ) : null}
                    </div>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={disabled || pullRequestStagePending}
                    onClick={() => onStagePullRequest(pullRequest.number)}
                  >
                    Create draft
                  </Button>
                </div>
              ))}
            </div>
          ) : null}
          {sourceMode === "prs" &&
          !pullRequestPending &&
          pullRequestCandidates.length === 0 ? (
            <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs text-[var(--text-tertiary)]">
              Load PRs to choose one pull request. RalphX uses PR metadata only;
              it does not read the full diff in this step.
            </div>
          ) : null}
          {pullRequestStageResult && sourceMode === "prs" ? (
            <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs leading-5 text-[var(--text-secondary)]">
              {pullRequestStageResult.skippedExisting
                ? `PR #${pullRequestStageResult.number} already has a draft in the Review Queue.`
                : `Created a draft for PR #${pullRequestStageResult.number}. Edit it before approval.`}
            </div>
          ) : null}
          {error || pullRequestError ? (
            <div
              role="alert"
              className="rounded-md border border-[var(--status-error)]/30 bg-[var(--status-error)]/10 px-3 py-2 text-xs text-[var(--status-error)]"
            >
              {(error ?? pullRequestError)?.message}
            </div>
          ) : null}
          <div className="flex justify-end">
            <Button
              type="button"
              onClick={
                sourceMode === "prs" ? onLoadPullRequests : onFindCandidates
              }
              disabled={actionDisabled}
            >
              <RefreshCw className={cn(actionPending && "animate-spin")} />
              {sourceMode === "prs" ? "Load PRs" : "Find candidates"}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ProjectSkillsExportDialog({
  disabled,
  exportEnabled,
  exportPreview,
  approvedCount,
  onExportEnabledChange,
  onPreview,
  onApply,
}: {
  disabled: boolean;
  exportEnabled: boolean;
  exportPreview: ProjectSkillExportResult | null;
  approvedCount: number;
  onExportEnabledChange: (enabled: boolean) => void;
  onPreview: () => void;
  onApply: () => void;
}) {
  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={disabled}
          className="w-full sm:w-auto"
        >
          <FileDown />
          Export...
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-[640px]">
        <DialogHeader>
          <div>
            <DialogTitle>Export approved skills</DialogTitle>
            <DialogDescription className="mt-1 leading-5">
              Export is project-scoped and opt-in. It writes only approved or
              pinned skills after preview.
            </DialogDescription>
          </div>
        </DialogHeader>
        <div className="grid gap-4 px-6 py-5">
          {approvedCount === 0 ? (
            <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs text-[var(--text-tertiary)]">
              Approve or pin at least one skill before export controls can
              write files.
            </div>
          ) : null}
          <ProjectSkillsExportControls
            disabled={disabled || approvedCount === 0}
            exportEnabled={exportEnabled}
            onExportEnabledChange={onExportEnabledChange}
            onPreview={onPreview}
            onApply={onApply}
          />
          {exportPreview ? (
            <ProjectSkillsExportSummary preview={exportPreview} />
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ProjectSkillEditDialog({
  skill,
  disabled,
  onUpdate,
}: {
  skill: ProjectSkill;
  disabled: boolean;
  onUpdate: (input: UpdateProjectSkillInput) => void;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState(skill.title);
  const [bucket, setBucket] = useState(skill.bucket);
  const [stage, setStage] = useState(skill.stage);
  const [compactGuidance, setCompactGuidance] = useState(skill.compactGuidance);
  const [bodyMarkdown, setBodyMarkdown] = useState(skill.bodyMarkdown);
  const [predictedEffect, setPredictedEffect] = useState(skill.predictedEffect ?? "");
  const [scopePaths, setScopePaths] = useState(skill.scopePaths.join("\n"));
  const [sourceSyncEnabled, setSourceSyncEnabled] = useState(
    skill.sourceSyncEnabled,
  );

  const resetForm = () => {
    setTitle(skill.title);
    setBucket(skill.bucket);
    setStage(skill.stage);
    setCompactGuidance(skill.compactGuidance);
    setBodyMarkdown(skill.bodyMarkdown);
    setPredictedEffect(skill.predictedEffect ?? "");
    setScopePaths(skill.scopePaths.join("\n"));
    setSourceSyncEnabled(skill.sourceSyncEnabled);
  };
  const canSave =
    title.trim().length > 0 &&
    compactGuidance.trim().length > 0 &&
    bodyMarkdown.trim().length > 0 &&
    predictedEffect.trim().length > 0;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        setOpen(nextOpen);
        if (nextOpen) {
          resetForm();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button type="button" size="sm" variant="outline" disabled={disabled}>
          <Pencil />
          Edit
        </Button>
      </DialogTrigger>
      <DialogContent className="max-w-[760px]">
        <DialogHeader>
          <div>
            <DialogTitle>Edit skill draft</DialogTitle>
            <DialogDescription className="mt-1 leading-5">
              Save edits to RalphX's internal skill copy. Source files are not
              overwritten from this dialog.
            </DialogDescription>
          </div>
        </DialogHeader>
        <div className="grid max-h-[70vh] gap-4 overflow-auto px-6 py-5">
          {skill.sourcePath ? (
            <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs leading-5 text-[var(--text-secondary)]">
              Source: <span className="font-medium">{skill.sourcePath}</span>.{" "}
              {sourceSyncEnabled
                ? "Track source is on: explicit source reloads may refresh this internal copy from the file."
                : "Snapshot mode is on: source reloads skip this internal copy unless tracking is enabled."}
            </div>
          ) : null}
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Title
            <Input
              aria-label="Skill title"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
            />
          </label>
          <div className="grid gap-3 sm:grid-cols-2">
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Bucket
              <ProjectSkillCategorySelect
                aria-label="Skill bucket"
                value={bucket}
                onChange={setBucket}
              />
            </label>
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Stage
              <ProjectSkillCategorySelect
                aria-label="Skill stage"
                value={stage}
                onChange={setStage}
              />
            </label>
          </div>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Compact guidance
            <Textarea
              aria-label="Skill compact guidance"
              className="min-h-20 text-xs"
              value={compactGuidance}
              onChange={(event) => setCompactGuidance(event.target.value)}
            />
          </label>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Full procedure
            <Textarea
              aria-label="Skill body"
              className="min-h-36 text-xs"
              value={bodyMarkdown}
              onChange={(event) => setBodyMarkdown(event.target.value)}
            />
          </label>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Expected effect
            <Textarea
              aria-label="Skill predicted effect"
              className="min-h-16 text-xs"
              value={predictedEffect}
              onChange={(event) => setPredictedEffect(event.target.value)}
            />
          </label>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Scope paths
            <Textarea
              aria-label="Skill scope paths"
              className="min-h-16 text-xs"
              placeholder="Optional, one path prefix per line"
              value={scopePaths}
              onChange={(event) => setScopePaths(event.target.value)}
            />
          </label>
          {skill.sourcePath ? (
            <div className="grid gap-2">
              <div className="text-xs font-medium uppercase text-[var(--text-tertiary)]">
                Source sync
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant={!sourceSyncEnabled ? "default" : "outline"}
                  aria-pressed={!sourceSyncEnabled}
                  onClick={() => setSourceSyncEnabled(false)}
                >
                  Snapshot copy
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant={sourceSyncEnabled ? "default" : "outline"}
                  aria-pressed={sourceSyncEnabled}
                  onClick={() => setSourceSyncEnabled(true)}
                >
                  Track source
                </Button>
              </div>
              <p className="text-xs leading-5 text-[var(--text-secondary)]">
                Track source only controls future RalphX reloads from the
                source folder. Saving here still updates RalphX only.
              </p>
            </div>
          ) : null}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => setOpen(false)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              disabled={disabled || !canSave}
              onClick={() => {
                onUpdate({
                  projectSkillId: skill.id,
                  title: title.trim(),
                  bucket,
                  stage,
                  scopePaths: scopePaths
                    .split(/\r?\n/)
                    .map((path) => path.trim())
                    .filter(Boolean),
                  compactGuidance: compactGuidance.trim(),
                  bodyMarkdown: bodyMarkdown.trim(),
                  predictedEffect: predictedEffect.trim(),
                  sourceSyncEnabled: skill.sourcePath ? sourceSyncEnabled : null,
                });
                setOpen(false);
              }}
            >
              Save changes
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ProjectSkillCategorySelect({
  "aria-label": ariaLabel,
  value,
  onChange,
}: {
  "aria-label": string;
  value: ProjectSkillCategory;
  onChange: (value: ProjectSkillCategory) => void;
}) {
  return (
    <select
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange(event.target.value as ProjectSkillCategory)}
      className="h-9 w-full rounded-md border border-[var(--border-default)] bg-[var(--bg-input)] px-3 py-1 text-sm text-[var(--text-primary)] shadow-sm outline-none transition-colors focus:border-[var(--border-focus)] focus:ring-1 focus:ring-[var(--border-focus)] disabled:cursor-not-allowed disabled:opacity-50"
    >
      {PROJECT_SKILL_CATEGORY_OPTIONS.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

function SkillMetricTile({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof ClipboardList;
  label: string;
  value: number | string;
  detail: string;
}) {
  return (
    <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs font-medium uppercase text-[var(--text-tertiary)]">
            {label}
          </div>
          <div className="mt-2 text-2xl font-semibold leading-none text-[var(--text-primary)]">
            {value}
          </div>
          <div className="mt-2 text-xs text-[var(--text-secondary)]">
            {detail}
          </div>
        </div>
        <div className="grid h-8 w-8 place-items-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)]">
          <Icon className="h-4 w-4" />
        </div>
      </div>
    </div>
  );
}
function parseImportManifest(value: string): ProjectSkillImportCandidate[] {
  const parsed = JSON.parse(value);
  const candidates = Array.isArray(parsed) ? parsed : parsed?.candidates;
  if (!Array.isArray(candidates)) {
    throw new Error("Import manifest must contain candidates.");
  }
  return candidates as ProjectSkillImportCandidate[];
}

function ImportPromotionPanel({
  disabled,
  importManifest,
  importPreview,
  projectDirectoryImport,
  projectDirectorySyncedCount,
  sourceImportRoots,
  sourceImportSyncEnabled,
  memoryPromotion,
  onImportManifestChange,
  onPreviewImport,
  onApplyImport,
  onSourceImportRootsChange,
  onSourceImportSyncEnabledChange,
  onApplyProjectDirectoryImport,
  onMemoryPromotionChange,
  onPromoteMemory,
}: {
  disabled: boolean;
  importManifest: string;
  importPreview: ProjectSkillImportPreviewResult | null;
  projectDirectoryImport: ProjectSkillImportPreviewResult | null;
  projectDirectorySyncedCount: number;
  sourceImportRoots: string[];
  sourceImportSyncEnabled: boolean;
  memoryPromotion: MemoryPromotionFormState;
  onImportManifestChange: (value: string) => void;
  onPreviewImport: () => void;
  onApplyImport: () => void;
  onSourceImportRootsChange: (roots: string[]) => void;
  onSourceImportSyncEnabledChange: (enabled: boolean) => void;
  onApplyProjectDirectoryImport: () => void;
  onMemoryPromotionChange: (value: MemoryPromotionFormState) => void;
  onPromoteMemory: () => void;
}) {
  const canSubmitImport = importManifest.trim().length > 0;
  const canPromoteMemory =
    memoryPromotion.memoryId.trim().length > 0 &&
    memoryPromotion.compactGuidance.trim().length > 0 &&
    memoryPromotion.bodyMarkdown.trim().length > 0 &&
    memoryPromotion.predictedEffect.trim().length > 0;

  const updatePromotion = (
    patch: Partial<MemoryPromotionFormState>,
  ): void => {
    onMemoryPromotionChange({ ...memoryPromotion, ...patch });
  };
  const toggleSourceRoot = (root: string): void => {
    const nextRoots = sourceImportRoots.includes(root)
      ? sourceImportRoots.filter((value) => value !== root)
      : [...sourceImportRoots, root];
    onSourceImportRootsChange(
      nextRoots.length > 0 ? nextRoots : [PROJECT_SKILL_SOURCE_ROOT_OPTIONS[0].id],
    );
  };

  return (
    <div className="grid gap-3 md:grid-cols-2">
      <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
        <div className="grid gap-3">
          <h3 className="text-xs font-semibold uppercase text-[var(--text-tertiary)]">
            Import draft skills
          </h3>
          <p className="text-xs text-[var(--text-secondary)]">
            Paste a JSON manifest from another source. Preview validates each
            row first; Add to review queue stages only eligible drafts for this
            project.
          </p>
          <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] p-3">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 flex-1">
                <div className="text-xs font-medium text-[var(--text-primary)]">
                  Existing project skill folders
                </div>
                <p className="mt-1 text-xs leading-5 text-[var(--text-secondary)]">
                  Load `SKILL.md` files from selected project-local folders into
                  the Review Queue. Imported rows are internal RalphX copies.
                </p>
              </div>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={disabled || sourceImportRoots.length === 0}
                onClick={onApplyProjectDirectoryImport}
              >
                <FileDown />
                Load folders
              </Button>
            </div>
            <div className="mt-3 grid gap-2">
              <div className="flex flex-wrap gap-2">
                {PROJECT_SKILL_SOURCE_ROOT_OPTIONS.map((option) => (
                  <Button
                    key={option.id}
                    type="button"
                    size="sm"
                    variant={
                      sourceImportRoots.includes(option.id) ? "default" : "outline"
                    }
                    aria-pressed={sourceImportRoots.includes(option.id)}
                    disabled={disabled}
                    onClick={() => toggleSourceRoot(option.id)}
                  >
                    {option.label}
                  </Button>
                ))}
              </div>
              <div className="flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant={!sourceImportSyncEnabled ? "default" : "outline"}
                  aria-pressed={!sourceImportSyncEnabled}
                  disabled={disabled}
                  onClick={() => onSourceImportSyncEnabledChange(false)}
                >
                  Snapshot copy
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant={sourceImportSyncEnabled ? "default" : "outline"}
                  aria-pressed={sourceImportSyncEnabled}
                  disabled={disabled}
                  onClick={() => onSourceImportSyncEnabledChange(true)}
                >
                  Track source
                </Button>
              </div>
              <p className="text-xs leading-5 text-[var(--text-secondary)]">
                Snapshot copies can be edited independently. Track source lets a
                future folder reload refresh the RalphX copy from the source
                file; it still never writes back to that file.
              </p>
            </div>
            {projectDirectoryImport ? (
              <div className="mt-3 flex flex-wrap gap-2 text-xs text-[var(--text-secondary)]">
                <Badge variant="secondary">
                  {projectDirectoryImport.eligibleCount} loaded
                </Badge>
                <Badge variant="outline">
                  {projectDirectoryImport.duplicateCount} duplicate
                </Badge>
                <Badge variant="outline">
                  {projectDirectoryImport.invalidCount} invalid
                </Badge>
                <Badge variant="outline">
                  {projectDirectorySyncedCount} synced
                </Badge>
              </div>
            ) : null}
          </div>
          <Textarea
            aria-label="Project skill import manifest"
            placeholder='{"candidates":[{"title":"...","bucket":"review","stage":"review","compactGuidance":"...","bodyMarkdown":"...","predictedEffect":"...","provenance":{},"sourceSnapshot":{}}]}'
            className="min-h-32 text-xs"
            value={importManifest}
            onChange={(event) => onImportManifestChange(event.target.value)}
          />
          {importPreview ? (
            <div className="flex flex-wrap gap-2 text-xs text-[var(--text-secondary)]">
              <Badge variant="secondary">{importPreview.eligibleCount} eligible</Badge>
              <Badge variant="outline">{importPreview.invalidCount} invalid</Badge>
              <Badge variant="outline">{importPreview.duplicateCount} duplicate</Badge>
            </div>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={disabled || !canSubmitImport}
              onClick={onPreviewImport}
            >
              Preview manifest
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={disabled || !canSubmitImport}
              onClick={onApplyImport}
            >
              Add to review queue
            </Button>
          </div>
        </div>
      </Card>

      <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
        <div className="grid gap-3">
          <h3 className="text-xs font-semibold uppercase text-[var(--text-tertiary)]">
            Draft from memory
          </h3>
          <p className="text-xs text-[var(--text-secondary)]">
            Use one saved memory only as provenance. The memory is not edited;
            you write the reusable procedure and send it to the Review Queue.
          </p>
          <div className="grid gap-2 sm:grid-cols-2">
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Memory ID
              <Input
                aria-label="Memory id"
                placeholder="memory_..."
                value={memoryPromotion.memoryId}
                onChange={(event) => updatePromotion({ memoryId: event.target.value })}
              />
            </label>
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Skill title
              <Input
                aria-label="Promoted skill title"
                placeholder="Defaults to memory title"
                value={memoryPromotion.title}
                onChange={(event) => updatePromotion({ title: event.target.value })}
              />
            </label>
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Bucket
              <ProjectSkillCategorySelect
                aria-label="Promoted skill bucket"
                value={memoryPromotion.bucket}
                onChange={(bucket) => updatePromotion({ bucket })}
              />
            </label>
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Stage
              <ProjectSkillCategorySelect
                aria-label="Promoted skill stage"
                value={memoryPromotion.stage}
                onChange={(stage) => updatePromotion({ stage })}
              />
            </label>
          </div>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Compact guidance
            <Textarea
              aria-label="Promoted skill guidance"
              placeholder="One or two sentences the agent should see during skill selection."
              className="min-h-20 text-xs"
              value={memoryPromotion.compactGuidance}
              onChange={(event) =>
                updatePromotion({ compactGuidance: event.target.value })
              }
            />
          </label>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Skill body
            <Textarea
              aria-label="Promoted skill body"
              placeholder="Reusable procedure, checks, examples, and boundaries. Do not paste raw memory facts."
              className="min-h-24 text-xs"
              value={memoryPromotion.bodyMarkdown}
              onChange={(event) => updatePromotion({ bodyMarkdown: event.target.value })}
            />
          </label>
          <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
            Predicted effect
            <Input
              aria-label="Promoted skill predicted effect"
              placeholder="Expected improvement, e.g. fewer repeated merge validation failures."
              value={memoryPromotion.predictedEffect}
              onChange={(event) => updatePromotion({ predictedEffect: event.target.value })}
            />
          </label>
          <div>
            <Button
              type="button"
              size="sm"
              disabled={disabled || !canPromoteMemory}
              onClick={onPromoteMemory}
            >
              Create draft skill
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}

function ReportCardSection({
  loading,
  cards,
}: {
  loading: boolean;
  cards: ProjectSkillReportCard[];
}) {
  return (
    <div className="grid gap-2">
      <h3 className="text-xs font-semibold uppercase text-[var(--text-tertiary)]">
        Report cards
      </h3>
      {loading ? (
        <Skeleton className="h-20 w-full" />
      ) : cards.length === 0 ? (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-4 py-6 text-sm text-[var(--text-secondary)]">
          No report cards yet.
        </div>
      ) : (
        <div className="grid gap-3">
          {cards.map((card) => (
            <ProjectSkillReportCardRow key={card.projectSkillId} card={card} />
          ))}
        </div>
      )}
    </div>
  );
}

function ProjectSkillReportCardRow({
  card,
}: {
  card: ProjectSkillReportCard;
}) {
  const evidenceLabel =
    card.evidenceLevel === "descriptive" ? "descriptive" : "low sample";

  return (
    <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate text-sm font-semibold text-[var(--text-primary)]">
              {card.title}
            </h3>
            <Badge variant="outline" className="text-xs">
              {card.bucket}
            </Badge>
            <Badge variant="secondary" className="text-xs">
              {evidenceLabel}
            </Badge>
            {card.agingStatus !== "active" ? (
              <Badge variant="outline" className="text-xs">
                {card.agingStatus}
              </Badge>
            ) : null}
          </div>
          <div className="mt-2 grid gap-1 text-xs text-[var(--text-secondary)] sm:grid-cols-2">
            <span>{card.usageCount} uses</span>
            <span>{card.linkedOutcomeCount} linked outcomes</span>
            <span>{card.succeededOutcomeCount} succeeded</span>
            <span>{card.failedOutcomeCount} failed</span>
          </div>
        </div>
        <div className="text-right text-xs text-[var(--text-tertiary)]">
          {card.lastUsedAt ? `${card.ageDays}d since use` : "not used"}
        </div>
      </div>
    </Card>
  );
}

interface SkillSectionProps {
  title: string;
  description: string;
  emptyMessage: string;
  skills: ProjectSkill[];
  renderSkill: (skill: ProjectSkill) => ReactNode;
}

function SkillSection({
  title,
  description,
  emptyMessage,
  skills,
  renderSkill,
}: SkillSectionProps) {
  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap items-end justify-between gap-2">
        <div>
          <h3 className="text-xs font-semibold uppercase text-[var(--text-tertiary)]">
            {title}
          </h3>
          <p className="mt-1 text-xs text-[var(--text-secondary)]">
            {description}
          </p>
        </div>
        <Badge variant="outline" className="text-xs">
          {skills.length}
        </Badge>
      </div>
      {skills.length === 0 ? (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-4 py-6 text-sm text-[var(--text-secondary)]">
          {emptyMessage}
        </div>
      ) : (
        <div className="grid gap-3">{skills.map(renderSkill)}</div>
      )}
    </div>
  );
}

interface ProjectSkillCandidateCardProps {
  skill: ProjectSkill;
  disabled: boolean;
  onApprove: () => void;
  onReject: () => void;
  onArchive: () => void;
  onUpdate: (input: UpdateProjectSkillInput) => void;
}

interface ProjectSkillApprovedCardProps {
  skill: ProjectSkill;
  disabled: boolean;
  onPin: () => void;
  onUnpin: () => void;
  onArchive: () => void;
  onUpdate: (input: UpdateProjectSkillInput) => void;
}

function ProjectSkillApprovedCard({
  skill,
  disabled,
  onPin,
  onUnpin,
  onArchive,
  onUpdate,
}: ProjectSkillApprovedCardProps) {
  return (
    <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="min-w-0 break-words text-sm font-semibold text-[var(--text-primary)]">
              {skill.title}
            </h3>
            <Badge variant="outline" className="text-xs">
              {skill.bucket}
            </Badge>
            {skill.pinned ? (
              <Badge variant="secondary" className="text-xs">
                pinned
              </Badge>
            ) : null}
          </div>
          <p className="mt-2 line-clamp-2 text-sm text-[var(--text-secondary)]">
            {skill.compactGuidance}
          </p>
          {skill.sourcePath ? <ProjectSkillSourceLine skill={skill} /> : null}
        </div>
        <div className="flex w-full flex-wrap gap-2 sm:w-auto sm:shrink-0 sm:justify-end">
          <ProjectSkillEditDialog
            skill={skill}
            disabled={disabled}
            onUpdate={onUpdate}
          />
          {skill.pinned ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={onUnpin}
              disabled={disabled}
            >
              <PinOff />
              Unpin
            </Button>
          ) : (
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={onPin}
              disabled={disabled}
            >
              <Pin />
              Pin
            </Button>
          )}
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={onArchive}
            disabled={disabled}
          >
            <Archive />
            Archive
          </Button>
        </div>
      </div>
    </Card>
  );
}

function ProjectSkillCandidateCard({
  skill,
  disabled,
  onApprove,
  onReject,
  onArchive,
  onUpdate,
}: ProjectSkillCandidateCardProps) {
  return (
    <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="min-w-0 break-words text-sm font-semibold text-[var(--text-primary)]">
              {skill.title}
            </h3>
            <Badge variant="outline" className="text-xs">
              {skill.bucket}
            </Badge>
            <Badge variant="secondary" className="text-xs">
              {skill.stage}
            </Badge>
          </div>
          <p className="mt-2 line-clamp-2 text-sm text-[var(--text-secondary)]">
            {skill.compactGuidance}
          </p>
          {skill.predictedEffect ? (
            <p className="mt-2 line-clamp-2 text-xs text-[var(--text-tertiary)]">
              {skill.predictedEffect}
            </p>
          ) : null}
          {skill.sourcePath ? <ProjectSkillSourceLine skill={skill} /> : null}
        </div>
        <div className="flex w-full flex-wrap gap-2 sm:w-auto sm:shrink-0 sm:justify-end">
          <ProjectSkillEditDialog
            skill={skill}
            disabled={disabled}
            onUpdate={onUpdate}
          />
          <Button
            type="button"
            size="sm"
            onClick={onApprove}
            disabled={disabled}
          >
            <Check />
            Approve
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={onReject}
            disabled={disabled}
          >
            <X />
            Reject
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={onArchive}
            disabled={disabled}
          >
            <Archive />
            Archive
          </Button>
        </div>
      </div>
    </Card>
  );
}

function ProjectSkillSourceLine({ skill }: { skill: ProjectSkill }) {
  return (
    <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-[var(--text-tertiary)]">
      <Badge variant="outline">
        {skill.sourceSyncEnabled ? "tracks source" : "snapshot"}
      </Badge>
      <span className="min-w-0 break-all">{skill.sourcePath}</span>
    </div>
  );
}
