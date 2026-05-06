import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { CheckSquare, Lightbulb, Loader2 } from "lucide-react";
import { ideationApi, toTaskProposal } from "@/api/ideation";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { TaskProposal } from "@/types/ideation";
import type { TaskProposalSummary } from "@/types/task-context";

interface TaskDetailProposalDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  proposalSummary: TaskProposalSummary | null;
}

interface ProposalDialogData {
  title: string;
  description: string | null;
  category: string | null;
  priority: string | null;
  complexity: string | null;
  priorityReason: string | null;
  steps: string[];
  acceptanceCriteria: string[];
  implementationNotes: string | null;
  planVersionAtCreation: number | null;
}

const COMPLEXITY_LABELS: Record<string, string> = {
  trivial: "Trivial",
  simple: "Simple",
  moderate: "Moderate",
  complex: "Complex",
  very_complex: "Very Complex",
};

function buildProposalDialogData(
  proposalSummary: TaskProposalSummary,
  proposal: TaskProposal | null
): ProposalDialogData {
  if (!proposal) {
    return {
      title: proposalSummary.title,
      description: proposalSummary.description,
      category: null,
      priority: null,
      complexity: null,
      priorityReason: null,
      steps: [],
      acceptanceCriteria: proposalSummary.acceptanceCriteria,
      implementationNotes: proposalSummary.implementationNotes,
      planVersionAtCreation: proposalSummary.planVersionAtCreation,
    };
  }

  return {
    title: proposal.title,
    description: proposal.description,
    category: proposal.category,
    priority: proposal.userPriority ?? proposal.suggestedPriority,
    complexity: COMPLEXITY_LABELS[proposal.estimatedComplexity] ?? proposal.estimatedComplexity,
    priorityReason: proposal.priorityReason,
    steps: proposal.steps,
    acceptanceCriteria: proposal.acceptanceCriteria,
    implementationNotes: proposalSummary.implementationNotes,
    planVersionAtCreation: proposal.planVersionAtCreation,
  };
}

export function TaskDetailProposalDialog({
  open,
  onOpenChange,
  proposalSummary,
}: TaskDetailProposalDialogProps) {
  const { data: proposalResponse, isLoading } = useQuery({
    queryKey: ["task-detail-context", "proposal", proposalSummary?.id] as const,
    queryFn: async () => ideationApi.proposals.get(proposalSummary!.id),
    enabled: open && Boolean(proposalSummary?.id),
    staleTime: 30_000,
  });

  const content = useMemo(
    () =>
      proposalSummary
        ? buildProposalDialogData(
            proposalSummary,
            proposalResponse ? toTaskProposal(proposalResponse) : null
          )
        : null,
    [proposalSummary, proposalResponse]
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex w-[min(760px,calc(100vw-48px))] max-h-[calc(100vh-48px)] max-w-none flex-col overflow-hidden p-0">
        <DialogHeader>
          <div>
            <DialogTitle>Source Proposal</DialogTitle>
            <DialogDescription>
              Proposal context linked to this task.
            </DialogDescription>
          </div>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
          {isLoading && proposalSummary ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="w-6 h-6 animate-spin text-text-primary/35" />
            </div>
          ) : content ? (
            <div className="space-y-6">
              <div>
                <div className="flex items-start gap-3">
                  <div className="mt-0.5 flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--accent-border)] bg-[color-mix(in_srgb,var(--accent-primary)_10%,transparent)]">
                    <Lightbulb className="w-4 h-4 text-[var(--accent-primary)]" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <h3 className="text-[1.125rem] font-semibold tracking-tight text-text-primary/90">
                      {content.title}
                    </h3>
                    <div className="mt-2 flex flex-wrap items-center gap-2">
                      {content.category && (
                        <span className="rounded-md bg-[var(--overlay-weak)] px-2 py-1 text-[0.6875rem] font-medium text-text-primary/50">
                          {content.category}
                        </span>
                      )}
                      {content.priority && (
                        <span className="rounded-md bg-[var(--status-warning-muted)] px-2 py-1 text-[0.6875rem] font-medium capitalize text-[var(--status-warning)]">
                          {content.priority}
                        </span>
                      )}
                      {content.complexity && (
                        <span className="rounded-md bg-[var(--overlay-weak)] px-2 py-1 text-[0.6875rem] font-medium text-text-primary/50">
                          {content.complexity}
                        </span>
                      )}
                      {content.planVersionAtCreation !== null && (
                        <span className="rounded-md bg-[var(--overlay-weak)] px-2 py-1 text-[0.6875rem] font-medium text-text-primary/50">
                          Plan v{content.planVersionAtCreation}
                        </span>
                      )}
                    </div>
                  </div>
                </div>
              </div>

              {content.description && (
                <section className="space-y-2">
                  <h4 className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-primary/40">
                    Description
                  </h4>
                  <p className="text-[0.8125rem] leading-relaxed text-text-primary/70">
                    {content.description}
                  </p>
                </section>
              )}

              {content.priorityReason && (
                <section className="space-y-2">
                  <h4 className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-primary/40">
                    Priority Rationale
                  </h4>
                  <p className="text-[0.8125rem] italic leading-relaxed text-text-primary/65">
                    "{content.priorityReason}"
                  </p>
                </section>
              )}

              {content.steps.length > 0 && (
                <section className="space-y-2">
                  <h4 className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-primary/40">
                    Implementation Steps
                  </h4>
                  <ol className="space-y-2">
                    {content.steps.map((step, index) => (
                      <li key={`${step}-${index}`} className="flex items-start gap-3">
                        <span className="mt-0.5 w-4 flex-shrink-0 text-right text-[0.6875rem] font-mono font-semibold text-[var(--accent-primary)]">
                          {index + 1}.
                        </span>
                        <span className="text-[0.8125rem] leading-snug text-text-primary/70">
                          {step}
                        </span>
                      </li>
                    ))}
                  </ol>
                </section>
              )}

              {content.acceptanceCriteria.length > 0 && (
                <section className="space-y-2">
                  <h4 className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-primary/40">
                    Acceptance Criteria
                  </h4>
                  <ul className="space-y-2">
                    {content.acceptanceCriteria.map((criterion, index) => (
                      <li key={`${criterion}-${index}`} className="flex items-start gap-2.5">
                        <CheckSquare className="mt-0.5 h-3.5 w-3.5 flex-shrink-0 text-[color-mix(in_srgb,var(--accent-primary)_50%,transparent)]" />
                        <span className="text-[0.8125rem] leading-snug text-text-primary/70">
                          {criterion}
                        </span>
                      </li>
                    ))}
                  </ul>
                </section>
              )}

              {content.implementationNotes && (
                <section className="space-y-2">
                  <h4 className="text-[0.6875rem] font-semibold uppercase tracking-wider text-text-primary/40">
                    Implementation Notes
                  </h4>
                  <p className="whitespace-pre-wrap text-[0.8125rem] leading-relaxed text-text-primary/70">
                    {content.implementationNotes}
                  </p>
                </section>
              )}
            </div>
          ) : (
            <div className="rounded-xl border border-[var(--overlay-weak)] bg-[var(--overlay-faint)] px-4 py-5 text-[0.8125rem] text-text-primary/55">
              The proposal details could not be loaded.
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
