/**
 * TaskContextPanel - Displays rich context for a task
 * Shows linked proposal summary, implementation plan preview, related artifacts, and context hints
 * Used by AgentsTaskDetailPanel when task has source_proposal_id or plan_artifact_id
 */

import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { taskContextApi } from "@/api/task-context";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { UsageInsightsCard } from "@/components/views/insights/UsageInsightsCard";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  FileText,
  Lightbulb,
  ChevronDown,
  ChevronUp,
  ExternalLink,
  Link2,
  AlertCircle,
} from "lucide-react";
import type { TaskContext, ArtifactSummary } from "@/types/task-context";
import { useState } from "react";
import { useTaskChatUsageStats } from "@/hooks/useTaskChatUsageStats";

interface TaskContextPanelProps {
  taskId: string;
  onViewArtifact?: ((artifactId: string) => void) | undefined;
}

const ARTIFACT_TYPE_ICONS: Record<string, typeof FileText> = {
  specification: FileText,
  research: FileText,
  design_doc: FileText,
  proposal: Lightbulb,
};

const ARTIFACT_TYPE_LABELS: Record<string, string> = {
  specification: "Specification",
  research: "Research",
  design_doc: "Design Doc",
  proposal: "Proposal",
};

function ArtifactTypeIcon({ type }: { type: string }) {
  const Icon = ARTIFACT_TYPE_ICONS[type] || FileText;
  return <Icon className="h-4 w-4 text-[var(--text-muted)]" />;
}

function ProposalSummarySection({ context }: { context: TaskContext }) {
  const [isOpen, setIsOpen] = useState(true);
  const proposal = context.sourceProposal;

  if (!proposal) return null;

  return (
    <Collapsible open={isOpen} onOpenChange={setIsOpen}>
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Lightbulb className="h-4 w-4 text-[var(--accent-primary)]" />
              <CardTitle className="text-sm font-medium">
                Source Proposal
              </CardTitle>
            </div>
            <CollapsibleTrigger asChild>
              <Button variant="ghost" size="sm">
                {isOpen ? (
                  <ChevronUp className="h-4 w-4" />
                ) : (
                  <ChevronDown className="h-4 w-4" />
                )}
              </Button>
            </CollapsibleTrigger>
          </div>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="pt-0 space-y-3">
            <div>
              <h4 className="text-sm font-semibold text-[var(--text-primary)]">
                {proposal.title}
              </h4>
              {proposal.description && (
                <div className="text-sm text-[var(--text-muted)] mt-1" style={{ wordBreak: "break-word" }}>
                  <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                    {proposal.description}
                  </ReactMarkdown>
                </div>
              )}
            </div>

            {proposal.acceptanceCriteria.length > 0 && (
              <div>
                <h5 className="text-xs font-medium text-[var(--text-secondary)] mb-1">
                  Acceptance Criteria
                </h5>
                <ul className="space-y-1">
                  {proposal.acceptanceCriteria.map((criteria, idx) => (
                    <li
                      key={idx}
                      className="text-xs text-[var(--text-muted)] flex items-start gap-2"
                    >
                      <span className="text-[var(--accent-primary)] mt-0.5">
                        •
                      </span>
                      <span>{criteria}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {proposal.implementationNotes && (
              <div>
                <h5 className="text-xs font-medium text-[var(--text-secondary)] mb-1">
                  Implementation Notes
                </h5>
                <p className="text-xs text-[var(--text-muted)] whitespace-pre-wrap">
                  {proposal.implementationNotes}
                </p>
              </div>
            )}

            {proposal.planVersionAtCreation && (
              <div className="text-xs text-[var(--text-muted)] flex items-center gap-1">
                <span>Plan version at creation:</span>
                <span className="font-mono">{proposal.planVersionAtCreation}</span>
              </div>
            )}
          </CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  );
}

function PlanArtifactSection({
  context,
  onViewArtifact,
}: {
  context: TaskContext;
  onViewArtifact?: ((artifactId: string) => void) | undefined;
}) {
  const [isOpen, setIsOpen] = useState(true);
  const plan = context.planArtifact;

  if (!plan) return null;

  return (
    <Collapsible open={isOpen} onOpenChange={setIsOpen}>
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <FileText className="h-4 w-4 text-[var(--accent-primary)]" />
              <CardTitle className="text-sm font-medium">
                Implementation Plan
              </CardTitle>
            </div>
            <CollapsibleTrigger asChild>
              <Button variant="ghost" size="sm">
                {isOpen ? (
                  <ChevronUp className="h-4 w-4" />
                ) : (
                  <ChevronDown className="h-4 w-4" />
                )}
              </Button>
            </CollapsibleTrigger>
          </div>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="pt-0 space-y-3">
            <div>
              <h4 className="text-sm font-semibold text-[var(--text-primary)]">
                {plan.title}
              </h4>
              <div className="text-xs text-[var(--text-muted)] mt-1">
                <span className="font-mono">v{plan.currentVersion}</span>
                <span className="mx-1">•</span>
                <span>{ARTIFACT_TYPE_LABELS[plan.artifactType] || plan.artifactType}</span>
              </div>
            </div>

            <div className="bg-[var(--bg-base)] border border-[var(--border)] rounded-md p-3">
              <p className="text-xs text-[var(--text-muted)] whitespace-pre-wrap font-mono leading-relaxed">
                {plan.contentPreview}
                {plan.contentPreview.length >= 500 && (
                  <span className="text-[var(--text-muted)]"> ...</span>
                )}
              </p>
            </div>

            {onViewArtifact && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => onViewArtifact(plan.id)}
                className="w-full"
              >
                <ExternalLink className="h-4 w-4 mr-2" />
                View Full Plan
              </Button>
            )}
          </CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  );
}

function RelatedArtifactsSection({
  context,
  onViewArtifact,
}: {
  context: TaskContext;
  onViewArtifact?: ((artifactId: string) => void) | undefined;
}) {
  const [isOpen, setIsOpen] = useState(true);
  const artifacts = context.relatedArtifacts;

  if (artifacts.length === 0) return null;

  return (
    <Collapsible open={isOpen} onOpenChange={setIsOpen}>
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Link2 className="h-4 w-4 text-[var(--accent-primary)]" />
              <CardTitle className="text-sm font-medium">
                Related Artifacts
              </CardTitle>
              <span className="text-xs text-[var(--text-muted)]">
                ({artifacts.length})
              </span>
            </div>
            <CollapsibleTrigger asChild>
              <Button variant="ghost" size="sm">
                {isOpen ? (
                  <ChevronUp className="h-4 w-4" />
                ) : (
                  <ChevronDown className="h-4 w-4" />
                )}
              </Button>
            </CollapsibleTrigger>
          </div>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="pt-0 space-y-2">
            {artifacts.map((artifact) => (
              <ArtifactItem
                key={artifact.id}
                artifact={artifact}
                onView={onViewArtifact}
              />
            ))}
          </CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  );
}

function ArtifactItem({
  artifact,
  onView,
}: {
  artifact: ArtifactSummary;
  onView?: ((artifactId: string) => void) | undefined;
}) {
  return (
    <div className="flex items-start gap-3 p-2 rounded-md border border-[var(--border)] bg-[var(--bg-base)] hover:bg-[var(--bg-subtle)] transition-colors">
      <div className="mt-0.5">
        <ArtifactTypeIcon type={artifact.artifactType} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-2">
          <h5 className="text-sm font-medium text-[var(--text-primary)] truncate">
            {artifact.title}
          </h5>
          {onView && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => onView(artifact.id)}
              className="h-6 px-2"
            >
              <ExternalLink className="h-3 w-3" />
            </Button>
          )}
        </div>
        <div className="text-xs text-[var(--text-muted)] mt-0.5">
          <span className="font-mono">v{artifact.currentVersion}</span>
          <span className="mx-1">•</span>
          <span>{ARTIFACT_TYPE_LABELS[artifact.artifactType] || artifact.artifactType}</span>
        </div>
        {artifact.contentPreview && (
          <p className="text-xs text-[var(--text-muted)] mt-1 line-clamp-2">
            {artifact.contentPreview}
          </p>
        )}
      </div>
    </div>
  );
}

function ContextHintsSection({ context }: { context: TaskContext }) {
  const hints = context.contextHints;

  if (hints.length === 0) return null;

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center gap-2">
          <AlertCircle className="h-4 w-4 text-[var(--accent-primary)]" />
          <CardTitle className="text-sm font-medium">Context Hints</CardTitle>
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <ul className="space-y-2">
          {hints.map((hint, idx) => (
            <li
              key={idx}
              className="text-xs text-[var(--text-muted)] flex items-start gap-2"
            >
              <span className="text-[var(--accent-primary)] mt-0.5">💡</span>
              <span>{hint}</span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function LoadingState() {
  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <Skeleton className="h-5 w-32" />
        </CardHeader>
        <CardContent>
          <Skeleton className="h-20 w-full" />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <Skeleton className="h-5 w-32" />
        </CardHeader>
        <CardContent>
          <Skeleton className="h-32 w-full" />
        </CardContent>
      </Card>
    </div>
  );
}

function EmptyState() {
  return (
    <Card>
      <CardContent className="flex flex-col items-center justify-center py-8 text-center">
        <AlertCircle className="h-8 w-8 text-[var(--text-muted)] mb-3" />
        <p className="text-sm text-[var(--text-muted)]">
          No context available for this task
        </p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          Task was not created from a proposal or does not have a linked plan
        </p>
      </CardContent>
    </Card>
  );
}

function ErrorState({ error }: { error: Error }) {
  return (
    <Card>
      <CardContent className="flex flex-col items-center justify-center py-8 text-center">
        <AlertCircle className="h-8 w-8 text-[var(--status-error)] mb-3" />
        <p className="text-sm text-[var(--text-primary)] font-medium">
          Failed to load task context
        </p>
        <p className="text-xs text-[var(--text-muted)] mt-1">
          {error.message}
        </p>
      </CardContent>
    </Card>
  );
}

export function TaskContextPanel({
  taskId,
  onViewArtifact,
}: TaskContextPanelProps) {
  const { data: usageStats } = useTaskChatUsageStats(taskId);
  const { data: context, isLoading, error } = useQuery({
    queryKey: ["task-context", taskId],
    queryFn: () => taskContextApi.getTaskContext(taskId),
  });

  if (isLoading) {
    return <LoadingState />;
  }

  if (error) {
    return <ErrorState error={error as Error} />;
  }

  if (!context) {
    return <EmptyState />;
  }

  // Check if there's any meaningful context to display
  const hasContext =
    context.sourceProposal ||
    context.planArtifact ||
    context.relatedArtifacts.length > 0 ||
    context.contextHints.length > 0;

  if (!hasContext) {
    return <EmptyState />;
  }

  return (
    <div className="space-y-4" data-testid="task-context-panel">
      {usageStats && <UsageInsightsCard stats={usageStats} />}
      <ProposalSummarySection context={context} />
      <PlanArtifactSection context={context} onViewArtifact={onViewArtifact} />
      <RelatedArtifactsSection context={context} onViewArtifact={onViewArtifact} />
      <ContextHintsSection context={context} />
    </div>
  );
}
