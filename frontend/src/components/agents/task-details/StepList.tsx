/**
 * StepList component
 *
 * Displays a list of task steps with loading states and empty state handling.
 * Supports editing and deletion when editable=true.
 */

import { ListChecks } from 'lucide-react';
import { StepItem } from './StepItem';
import { Skeleton } from '@/components/ui/skeleton';
import { useTaskSteps } from '@/hooks/useTaskSteps';
import { useStepMutations } from '@/hooks/useStepMutations';

interface StepListProps {
  taskId: string;
  editable?: boolean;
  /** Hide completion notes (useful for historical views before execution) */
  hideCompletionNotes?: boolean;
}

/**
 * StepList Component
 *
 * Fetches and displays task steps using StepItem components.
 * Shows loading skeleton while fetching and empty state when no steps exist.
 *
 * @example
 * ```tsx
 * <StepList taskId="task-123" editable={true} />
 * ```
 */
export function StepList({ taskId, editable = false, hideCompletionNotes = false }: StepListProps) {
  const { data: steps, isLoading, isError } = useTaskSteps(taskId);
  const { skip: skipStep } = useStepMutations(taskId);

  // Loading state
  if (isLoading) {
    return (
      <div className="space-y-3">
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-16 w-full" />
      </div>
    );
  }

  // Error state
  if (isError) {
    return (
      <div className="flex flex-col items-center justify-center py-8 text-center">
        <p className="text-sm text-status-error">Failed to load steps</p>
      </div>
    );
  }

  // Empty state
  if (!steps || steps.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-8 text-center">
        <ListChecks className="h-12 w-12 text-text-muted mb-3" />
        <h3 className="text-sm font-medium text-text-primary mb-1">No steps yet</h3>
        <p className="text-sm text-text-muted">
          {editable
            ? 'Add steps to track progress on this task'
            : 'Steps will appear here during execution'}
        </p>
      </div>
    );
  }

  // Steps list - minimal
  return (
    <div className="space-y-0">
      {steps.map((step, index) => {
        const props = {
          step,
          index,
          editable,
          hideCompletionNote: hideCompletionNotes,
          ...(editable && { onSkip: (stepId: string) => skipStep.mutate({ stepId, reason: "Skipped by user" }) }),
        };
        return (
          <div key={step.id}>
            {index > 0 && (
              <div
                className="h-px mx-3"
                style={{ backgroundColor: 'var(--overlay-faint)' }}
              />
            )}
            <StepItem {...props} />
          </div>
        );
      })}
    </div>
  );
}
