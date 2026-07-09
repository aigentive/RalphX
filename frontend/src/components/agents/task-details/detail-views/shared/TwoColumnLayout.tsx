/**
 * TwoColumnLayout - Compatibility wrapper for the Agents task detail shell.
 *
 * The Agents right panel is narrow, so this now renders a single ordered column:
 * task summary, historical context, stage body, evidence, context, then actions.
 */

import { type ReactNode } from "react";
import { SectionTitle } from "./SectionTitle";
import { DescriptionBlock } from "./DescriptionBlock";
import { useTaskDetailContextModel } from "./TaskDetailContext";
import { TaskContextRail } from "./TaskDetailContextRail";

interface TwoColumnLayoutProps {
  description: string | null | undefined;
  children: ReactNode;
  testId?: string;
  leftRail?: ReactNode;
  evidence?: ReactNode;
  context?: ReactNode;
  actions?: ReactNode;
}

export function TwoColumnLayout({
  description,
  children,
  testId,
  leftRail,
  evidence,
  context,
  actions,
}: TwoColumnLayoutProps) {
  const detailContext = useTaskDetailContextModel();
  const summary =
    leftRail ??
    (detailContext ? (
      <TaskContextRail
        model={detailContext}
        fallbackDescription={description}
        showMerge={false}
      />
    ) : (
      <div className="space-y-2">
        <SectionTitle>Description</SectionTitle>
        <DescriptionBlock description={description} />
      </div>
    ));
  const defaultContext =
    detailContext?.merge ? (
      <TaskContextRail
        model={detailContext}
        fallbackDescription={description}
        showTask={false}
        showHistorical={false}
      />
    ) : null;
  const contextSection = context ?? defaultContext;

  return (
    <div
      data-testid={testId}
      className="min-h-0 space-y-6"
    >
      <div data-testid="task-detail-summary" className="min-w-0">
        {summary}
      </div>

      <div data-testid="task-detail-stage-body" className="min-w-0 space-y-6">
        {children}
      </div>

      {evidence && (
        <div data-testid="task-detail-evidence" className="min-w-0 space-y-6">
          {evidence}
        </div>
      )}

      {contextSection && (
        <div data-testid="task-detail-context" className="min-w-0">
          {contextSection}
        </div>
      )}

      {actions && (
        <div data-testid="task-detail-actions" className="min-w-0">
          {actions}
        </div>
      )}
    </div>
  );
}
