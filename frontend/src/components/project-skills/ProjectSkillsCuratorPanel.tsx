import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, Check, Pin, PinOff, RefreshCw, X } from "lucide-react";
import type { ReactNode } from "react";
import { useState } from "react";

import {
  projectSkillsApi,
  type ProjectSkill,
  type ProjectSkillExportResult,
  type ProjectSkillReportCard,
} from "@/api/project-skills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
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

export function ProjectSkillsCuratorPanel({
  projectId,
  className,
}: ProjectSkillsCuratorPanelProps) {
  const queryClient = useQueryClient();
  const [exportPreview, setExportPreview] =
    useState<ProjectSkillExportResult | null>(null);
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
    applyExportMutation.isPending;
  const error =
    stagedQuery.error ??
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
    applyExportMutation.error;
  const exportEnabled = settingsQuery.data?.exportEnabled ?? false;

  return (
    <section
      className={cn("flex min-h-0 flex-col gap-3", className)}
      aria-label="Learned skills"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-sm font-semibold text-[var(--text-primary)]">
            Learned skills
          </h2>
          <div className="mt-1 text-xs text-[var(--text-secondary)]">
            {stagedSkills.length} staged, {approvedSkills.length} approved
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ProjectSkillsExportControls
            disabled={isBusy || approvedSkills.length === 0}
            exportEnabled={exportEnabled}
            onExportEnabledChange={(enabled) =>
              updateSettingsMutation.mutate(enabled)
            }
            onPreview={() => previewExportMutation.mutate()}
            onApply={() => applyExportMutation.mutate()}
          />
          <Button
            type="button"
            size="sm"
            onClick={() => distillMutation.mutate()}
            disabled={isBusy}
          >
            <RefreshCw
              className={cn(distillMutation.isPending && "animate-spin")}
            />
            Distill
          </Button>
        </div>
      </div>

      {error ? (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error)]/30 bg-[var(--status-error)]/10 px-3 py-2 text-sm text-[var(--status-error)]"
        >
          {error.message}
        </div>
      ) : null}

      {exportPreview ? (
        <ProjectSkillsExportSummary preview={exportPreview} />
      ) : null}

      {stagedQuery.isLoading || approvedQuery.isLoading ? (
        <div className="grid gap-3">
          <Skeleton className="h-28 w-full" />
          <Skeleton className="h-28 w-full" />
        </div>
      ) : (
        <div className="grid gap-4">
          <SkillSection
            title="Staged"
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
          <SkillSection
            title="Approved"
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
          <ReportCardSection
            loading={reportCardsQuery.isLoading}
            cards={reportCards}
          />
        </div>
      )}
    </section>
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
  emptyMessage: string;
  skills: ProjectSkill[];
  renderSkill: (skill: ProjectSkill) => ReactNode;
}

function SkillSection({
  title,
  emptyMessage,
  skills,
  renderSkill,
}: SkillSectionProps) {
  return (
    <div className="grid gap-2">
      <h3 className="text-xs font-semibold uppercase text-[var(--text-tertiary)]">
        {title}
      </h3>
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
