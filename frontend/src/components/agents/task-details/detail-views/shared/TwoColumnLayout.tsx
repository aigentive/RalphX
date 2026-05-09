/**
 * TwoColumnLayout - Consistent layout for task detail views
 *
 * Left column: Description (fixed)
 * Right column: Main content (varies by view)
 */

import { SectionTitle } from "./SectionTitle";
import { DescriptionBlock } from "./DescriptionBlock";
import { useTaskDetailContextModel } from "./TaskDetailContext";
import { TaskContextRail } from "./TaskDetailContextRail";

interface TwoColumnLayoutProps {
  description: string | null | undefined;
  children: React.ReactNode;
  testId?: string;
  leftRail?: React.ReactNode;
}

export function TwoColumnLayout({
  description,
  children,
  testId,
  leftRail,
}: TwoColumnLayoutProps) {
  const detailContext = useTaskDetailContextModel();
  const rail =
    leftRail ??
    (detailContext ? (
      <TaskContextRail
        model={detailContext}
        fallbackDescription={description}
      />
    ) : (
      <div className="space-y-2">
        <SectionTitle>Description</SectionTitle>
        <DescriptionBlock description={description} />
      </div>
    ));

  return (
    <div
      data-testid={testId}
      className="grid min-h-0 grid-cols-1 gap-6 xl:grid-cols-[minmax(320px,360px)_minmax(0,1fr)]"
    >
      {/* Left column - common task context */}
      <div className="min-w-0">{rail}</div>

      {/* Right column - Main content */}
      <div className="space-y-6 min-w-0">{children}</div>
    </div>
  );
}
