/**
 * StepItem component
 *
 * Displays a single task step with status icon, title, description, and completion note.
 * Containers stay neutral while status icons carry subtle state color.
 */

import { Circle, Loader2, CheckCircle2, MinusCircle, XCircle, SkipForward } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { TaskStep, TaskStepStatus } from '@/types/task-step';

interface StepItemProps {
  step: TaskStep;
  index: number;
  editable?: boolean;
  /** Hide completion notes (useful for historical views before execution) */
  hideCompletionNote?: boolean;
  onUpdate?: (step: TaskStep) => void;
  onSkip?: (stepId: string) => void;
}

/**
 * Render the appropriate status icon
 */
function StatusIcon({ status, className, style }: { status: TaskStepStatus; className: string; style?: React.CSSProperties }) {
  switch (status) {
    case 'pending':
      return <Circle className={className} style={style} />;
    case 'in_progress':
      return <Loader2 className={`${className} animate-spin`} style={style} />;
    case 'completed':
      return <CheckCircle2 className={className} style={style} />;
    case 'skipped':
      return <MinusCircle className={className} style={style} />;
    case 'failed':
      return <XCircle className={className} style={style} />;
    case 'cancelled':
      return <XCircle className={className} style={style} />;
  }
}

/**
 * Get color for step status icon.
 */
function getStatusColor(status: TaskStepStatus): string {
  switch (status) {
    case 'pending':
      return 'var(--text-muted)';
    case 'in_progress':
      return 'color-mix(in srgb, var(--accent-primary) 82%, var(--text-muted))';
    case 'completed':
      return 'color-mix(in srgb, var(--status-success) 78%, var(--text-muted))';
    case 'skipped':
      return 'var(--text-muted)';
    case 'failed':
      return 'color-mix(in srgb, var(--status-error) 78%, var(--text-muted))';
    case 'cancelled':
      return 'var(--text-muted)';
  }
}

/**
 * Get container styles based on step status (Tahoe design - minimal, no borders)
 */
function getContainerStyles(status: TaskStepStatus): React.CSSProperties {
  const base: React.CSSProperties = {
    display: 'flex',
    alignItems: 'flex-start',
    gap: '10px',
    padding: '10px 12px',
    borderRadius: '6px',
    transition: 'all 150ms ease',
  };

  switch (status) {
    case 'skipped':
      return {
        ...base,
        backgroundColor: 'transparent',
        opacity: 0.5,
      };
    case 'in_progress':
    case 'failed':
      return {
        ...base,
        backgroundColor: 'var(--overlay-faint)',
      };
    default:
      return {
        ...base,
        backgroundColor: 'transparent',
      };
  }
}

/**
 * StepItem Component
 *
 * Renders a single step in a task's step list with appropriate visual styling
 * based on its status. Supports editing and deletion when editable=true.
 *
 * @example
 * ```tsx
 * <StepItem
 *   step={step}
 *   index={0}
 *   editable={true}
 *   onSkip={(id) => handleSkip(id)}
 * />
 * ```
 */
export function StepItem({ step, index, editable = false, hideCompletionNote = false, onSkip }: StepItemProps) {
  const iconColor = getStatusColor(step.status);
  const containerStyles = getContainerStyles(step.status);
  const isSkipped = step.status === 'skipped';

  return (
    <div style={containerStyles}>
      {/* Status Icon */}
      <div className="flex-shrink-0 mt-0.5">
        <StatusIcon status={step.status} className="h-4 w-4" style={{ color: iconColor }} />
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        {/* Step number and title */}
        <div
          className="flex items-baseline gap-1"
          style={{ textDecoration: isSkipped ? 'line-through' : 'none' }}
        >
          <span
            style={{
              fontSize: '0.8125rem',
              fontWeight: 400,
              color: 'var(--text-muted)',
            }}
          >
            Step {index + 1}:
          </span>
          <span
            style={{
              fontSize: '0.8125rem',
              fontWeight: 400,
              color: 'var(--text-secondary)',
              marginLeft: '4px',
            }}
          >
            {step.title}
          </span>
        </div>

        {/* Description */}
        {step.description && (
          <p
            style={{
              marginTop: '2px',
              fontSize: '0.75rem',
              color: 'var(--text-muted)',
              lineHeight: 1.5,
              textDecoration: isSkipped ? 'line-through' : 'none',
            }}
          >
            {step.description}
          </p>
        )}

        {/* Completion note (shown for completed, skipped, failed - hidden in historical views) */}
        {step.completionNote && !hideCompletionNote && (
          <p
            style={{
              marginTop: '4px',
              fontSize: '0.6875rem',
              color: 'var(--text-muted)',
              fontStyle: 'italic',
              lineHeight: 1.5,
            }}
          >
            {step.completionNote}
          </p>
        )}
      </div>

      {/* Skip button (only for editable pending steps) */}
      {editable && step.status === 'pending' && onSkip && (
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onSkip(step.id)}
          className="flex-shrink-0 h-7 w-7 p-0"
          aria-label="Skip step"
        >
          <SkipForward className="h-3.5 w-3.5" style={{ color: 'var(--text-muted)' }} />
        </Button>
      )}
    </div>
  );
}
