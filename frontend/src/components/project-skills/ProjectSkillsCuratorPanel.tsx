import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, Check, RefreshCw, X } from "lucide-react";

import { projectSkillsApi, type ProjectSkill } from "@/api/project-skills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

interface ProjectSkillsCuratorPanelProps {
  projectId: string;
  className?: string;
}

const stagedSkillsKey = (projectId: string) => [
  "project-skills",
  projectId,
  "staged",
] as const;

export function ProjectSkillsCuratorPanel({
  projectId,
  className,
}: ProjectSkillsCuratorPanelProps) {
  const queryClient = useQueryClient();
  const queryKey = stagedSkillsKey(projectId);

  const stagedQuery = useQuery({
    queryKey,
    queryFn: () =>
      projectSkillsApi.list({
        projectId,
        status: "staged",
        includeArchived: false,
      }),
  });

  const invalidateStaged = () => queryClient.invalidateQueries({ queryKey });

  const distillMutation = useMutation({
    mutationFn: () => projectSkillsApi.distill({ projectId, limit: 10 }),
    onSuccess: invalidateStaged,
  });

  const approveMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.approve(skillId),
    onSuccess: invalidateStaged,
  });

  const rejectMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.reject(skillId),
    onSuccess: invalidateStaged,
  });

  const archiveMutation = useMutation({
    mutationFn: (skillId: string) => projectSkillsApi.archive(skillId),
    onSuccess: invalidateStaged,
  });

  const skills = stagedQuery.data ?? [];
  const isBusy =
    distillMutation.isPending ||
    approveMutation.isPending ||
    rejectMutation.isPending ||
    archiveMutation.isPending;
  const error =
    stagedQuery.error ??
    distillMutation.error ??
    approveMutation.error ??
    rejectMutation.error ??
    archiveMutation.error;

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
            {skills.length} staged
          </div>
        </div>
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

      {error ? (
        <div
          role="alert"
          className="rounded-md border border-[var(--status-error)]/30 bg-[var(--status-error)]/10 px-3 py-2 text-sm text-[var(--status-error)]"
        >
          {error.message}
        </div>
      ) : null}

      {stagedQuery.isLoading ? (
        <div className="grid gap-3">
          <Skeleton className="h-28 w-full" />
          <Skeleton className="h-28 w-full" />
        </div>
      ) : skills.length === 0 ? (
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-4 py-6 text-sm text-[var(--text-secondary)]">
          No staged learned skills.
        </div>
      ) : (
        <div className="grid gap-3">
          {skills.map((skill) => (
            <ProjectSkillCandidateCard
              key={skill.id}
              skill={skill}
              disabled={isBusy}
              onApprove={() => approveMutation.mutate(skill.id)}
              onReject={() => rejectMutation.mutate(skill.id)}
              onArchive={() => archiveMutation.mutate(skill.id)}
            />
          ))}
        </div>
      )}
    </section>
  );
}

interface ProjectSkillCandidateCardProps {
  skill: ProjectSkill;
  disabled: boolean;
  onApprove: () => void;
  onReject: () => void;
  onArchive: () => void;
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
