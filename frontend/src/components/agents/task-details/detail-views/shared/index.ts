/**
 * Shared components for detail-views
 *
 * Common components used across multiple state-specific task detail views.
 */

export { SectionTitle } from "./SectionTitle";
export { ReviewTimeline } from "./ReviewTimeline";
export type { ReviewTimelineProps } from "./ReviewTimeline";
export { DetailCard } from "./DetailCard";
export { StatusBanner } from "./StatusBanner";
export { StatusPill } from "./StatusPill";
export { ProgressIndicator } from "./ProgressIndicator";
export { DescriptionBlock } from "./DescriptionBlock";
export { TwoColumnLayout } from "./TwoColumnLayout";
export { TaskMetricsCard } from "./TaskMetricsCard";
export { ChangeReviewSection, CommitSummaryCard } from "./ChangeReviewSection";
export { PlanMergeContextCard, PlanMergeContextSection } from "./PlanMergeContextSection";
export { TaskContextRail } from "./TaskDetailContextRail";
export { TaskDetailContextProvider } from "./TaskDetailContextProvider";
export {
  useTaskDetailContextModel,
  type TaskDetailContextModel,
  type TaskDetailViewMode,
} from "./TaskDetailContext";
