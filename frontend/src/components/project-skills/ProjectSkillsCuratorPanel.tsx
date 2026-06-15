import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  Check,
  ClipboardList,
  FileDown,
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
  projectSkillsApi,
  type ProjectSkillImportCandidate,
  type ProjectSkillImportPreviewResult,
  type ProjectSkill,
  type ProjectSkillExportResult,
  type ProjectSkillReportCard,
} from "@/api/project-skills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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

interface MemoryPromotionFormState {
  memoryId: string;
  title: string;
  bucket: string;
  stage: string;
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string;
}

export function ProjectSkillsCuratorPanel({
  projectId,
  className,
}: ProjectSkillsCuratorPanelProps) {
  const queryClient = useQueryClient();
  const [exportPreview, setExportPreview] =
    useState<ProjectSkillExportResult | null>(null);
  const [importManifest, setImportManifest] = useState("");
  const [importPreview, setImportPreview] =
    useState<ProjectSkillImportPreviewResult | null>(null);
  const [memoryPromotion, setMemoryPromotion] = useState({
    memoryId: "",
    title: "",
    bucket: "review",
    stage: "review",
    compactGuidance: "",
    bodyMarkdown: "",
    predictedEffect: "",
  });
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
    mutationFn: () => projectSkillsApi.distill({ projectId, limit: 10 }),
    onSuccess: invalidateSkills,
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

  const promoteMemoryMutation = useMutation({
    mutationFn: () =>
      projectSkillsApi.promoteMemory({
        projectId,
        memoryId: memoryPromotion.memoryId.trim(),
        title: memoryPromotion.title.trim() || null,
        bucket: memoryPromotion.bucket.trim(),
        stage: memoryPromotion.stage.trim(),
        compactGuidance: memoryPromotion.compactGuidance,
        bodyMarkdown: memoryPromotion.bodyMarkdown,
        predictedEffect: memoryPromotion.predictedEffect,
      }),
    onMutate: () => setLocalError(null),
    onSuccess: () => {
      invalidateSkills();
      setMemoryPromotion({
        memoryId: "",
        title: "",
        bucket: "review",
        stage: "review",
        compactGuidance: "",
        bodyMarkdown: "",
        predictedEffect: "",
      });
    },
    onError: (error) => setLocalError(error.message),
  });

  const stagedSkills = stagedQuery.data ?? [];
  const approvedSkills = approvedQuery.data ?? [];
  const reportCards = reportCardsQuery.data?.cards ?? [];
  const isBusy =
    distillMutation.isPending ||
    approveMutation.isPending ||
    rejectMutation.isPending ||
    archiveMutation.isPending ||
    pinMutation.isPending ||
    unpinMutation.isPending ||
    updateSettingsMutation.isPending ||
    previewExportMutation.isPending ||
    applyExportMutation.isPending ||
    previewImportMutation.isPending ||
    applyImportMutation.isPending ||
    promoteMemoryMutation.isPending;
  const error =
    localError != null
      ? new Error(localError)
      : stagedQuery.error ??
        approvedQuery.error ??
        settingsQuery.error ??
        reportCardsQuery.error ??
        distillMutation.error ??
        approveMutation.error ??
        rejectMutation.error ??
        archiveMutation.error ??
        pinMutation.error ??
        unpinMutation.error ??
        updateSettingsMutation.error ??
        previewExportMutation.error ??
        applyExportMutation.error ??
        previewImportMutation.error ??
        applyImportMutation.error ??
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

      <div className="grid gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold text-[var(--text-primary)]">
              Skill Operations
            </h2>
            <p className="mt-1 text-xs leading-5 text-[var(--text-secondary)]">
              Find reusable procedures from completed task, conversation, and
              agent workspace outcomes, then review them before agents can use
              them. Export remains opt-in.
            </p>
          </div>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                size="sm"
                onClick={() => distillMutation.mutate()}
                disabled={isBusy}
              >
                <RefreshCw
                  className={cn(distillMutation.isPending && "animate-spin")}
                />
                Find candidates
              </Button>
            </TooltipTrigger>
            <TooltipContent className="max-w-[280px] leading-5">
              Scans recorded outcomes from completed work, proposes reusable
              skill candidates, and leaves them in the review queue. It does
              not approve or inject skills automatically.
            </TooltipContent>
          </Tooltip>
        </div>
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs leading-5 text-[var(--text-secondary)]">
          <span className="font-medium text-[var(--text-primary)]">
            What Find candidates does:
          </span>{" "}
          scans stored task, conversation, and agent workspace outcomes for
          repeated procedural lessons and stages draft skills for human
          approval.
        </div>
      </div>

      <Tabs defaultValue="review" className="grid gap-4">
        <TabsList className="h-auto justify-start rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-1">
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
                />
              )}
            />
          )}
        </TabsContent>

        <TabsContent value="approved" className="mt-0 grid gap-4">
          <div className="grid gap-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
            <div className="min-w-0">
              <h2 className="text-sm font-semibold text-[var(--text-primary)]">
                Export approved skills
              </h2>
              <p className="mt-1 text-xs leading-5 text-[var(--text-secondary)]">
                Export is project-scoped and opt-in. It writes only approved or
                pinned skills after preview.
              </p>
            </div>
            <ProjectSkillsExportControls
              disabled={isBusy || approvedSkills.length === 0}
              exportEnabled={exportEnabled}
              onExportEnabledChange={(enabled) =>
                updateSettingsMutation.mutate(enabled)
              }
              onPreview={() => previewExportMutation.mutate()}
              onApply={() => applyExportMutation.mutate()}
            />
            {approvedSkills.length === 0 ? (
              <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-2 text-xs text-[var(--text-tertiary)]">
                Approve or pin at least one skill before export controls can
                write files.
              </div>
            ) : null}
          </div>
          {exportPreview ? (
            <ProjectSkillsExportSummary preview={exportPreview} />
          ) : null}
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
            memoryPromotion={memoryPromotion}
            onImportManifestChange={setImportManifest}
            onPreviewImport={() => previewImportMutation.mutate()}
            onApplyImport={() => applyImportMutation.mutate()}
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
  memoryPromotion,
  onImportManifestChange,
  onPreviewImport,
  onApplyImport,
  onMemoryPromotionChange,
  onPromoteMemory,
}: {
  disabled: boolean;
  importManifest: string;
  importPreview: ProjectSkillImportPreviewResult | null;
  memoryPromotion: MemoryPromotionFormState;
  onImportManifestChange: (value: string) => void;
  onPreviewImport: () => void;
  onApplyImport: () => void;
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
              <Input
                aria-label="Promoted skill bucket"
                placeholder="review"
                value={memoryPromotion.bucket}
                onChange={(event) => updatePromotion({ bucket: event.target.value })}
              />
            </label>
            <label className="grid gap-1 text-xs text-[var(--text-secondary)]">
              Stage
              <Input
                aria-label="Promoted skill stage"
                placeholder="review"
                value={memoryPromotion.stage}
                onChange={(event) => updatePromotion({ stage: event.target.value })}
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
}

interface ProjectSkillApprovedCardProps {
  skill: ProjectSkill;
  disabled: boolean;
  onPin: () => void;
  onUnpin: () => void;
  onArchive: () => void;
}

function ProjectSkillApprovedCard({
  skill,
  disabled,
  onPin,
  onUnpin,
  onArchive,
}: ProjectSkillApprovedCardProps) {
  return (
    <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate text-sm font-semibold text-[var(--text-primary)]">
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
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
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
}: ProjectSkillCandidateCardProps) {
  return (
    <Card className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate text-sm font-semibold text-[var(--text-primary)]">
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
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
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
