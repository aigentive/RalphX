/**
 * Shared components re-export for backward compatibility
 *
 * Components have been moved to ./shared/ directory.
 * This file re-exports them to maintain existing import paths.
 */

export { SectionTitle } from "./shared/SectionTitle";
export { ReviewTimeline } from "./shared/ReviewTimeline";
export type { ReviewTimelineProps } from "./shared/ReviewTimeline";
export { DetailCard } from "./shared/DetailCard";
export { StatusBanner } from "./shared/StatusBanner";
export { StatusPill } from "./shared/StatusPill";
export { ProgressIndicator } from "./shared/ProgressIndicator";
export { DescriptionBlock } from "./shared/DescriptionBlock";
export { TwoColumnLayout } from "./shared/TwoColumnLayout";
export { TaskMetricsCard } from "./shared/TaskMetricsCard";
export { ChangeReviewSection, CommitSummaryCard } from "./shared/ChangeReviewSection";
export { PlanMergeContextCard, PlanMergeContextSection } from "./shared/PlanMergeContextSection";
export { TaskContextRail } from "./shared/TaskDetailContextRail";
