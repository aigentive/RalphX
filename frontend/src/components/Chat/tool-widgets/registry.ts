/**
 * Tool Call Widget Registry
 *
 * Maps tool names to specialized React widget components.
 * ToolCallIndicator checks this registry before falling back to the generic renderer.
 * Harness/session identity is rendered by the chat chrome, not hard-coded into individual widgets.
 *
 * To register a new widget:
 *   1. Create src/components/Chat/tool-widgets/YourWidget.tsx implementing ToolCallWidgetProps
 *   2. Import and add to TOOL_CALL_WIDGETS below
 */

import {
  type ComponentType,
  type LazyExoticComponent,
} from "react";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import type { ToolCallWidgetProps } from "./shared";
import { getToolCallLookupCandidates } from "./tool-name";

/** Registry type: tool name (lowercase) → React component */
export type ToolCallWidgetComponent =
  | ComponentType<ToolCallWidgetProps>
  | LazyExoticComponent<ComponentType<ToolCallWidgetProps>>;
export type ToolCallWidgetRegistry = Record<string, ToolCallWidgetComponent>;

function lazyWidget(
  loader: () => Promise<{ default: ComponentType<ToolCallWidgetProps> }>
): LazyExoticComponent<ComponentType<ToolCallWidgetProps>> {
  return lazyWithRetry(loader);
}

const StepIndicator = lazyWidget(() =>
  import("./StepIndicator").then((module) => ({ default: module.StepIndicator }))
);
const ContextWidget = lazyWidget(() =>
  import("./ContextWidget").then((module) => ({ default: module.ContextWidget }))
);
const StepsManifestWidget = lazyWidget(() =>
  import("./StepsManifestWidget").then((module) => ({ default: module.StepsManifestWidget }))
);
const IssuesSummaryWidget = lazyWidget(() =>
  import("./IssuesSummaryWidget").then((module) => ({ default: module.IssuesSummaryWidget }))
);
const ArtifactWidget = lazyWidget(() =>
  import("./ArtifactWidget").then((module) => ({ default: module.ArtifactWidget }))
);
const ReviewWidget = lazyWidget(() =>
  import("./ReviewWidget").then((module) => ({ default: module.ReviewWidget }))
);
const MergeWidget = lazyWidget(() =>
  import("./MergeWidget").then((module) => ({ default: module.MergeWidget }))
);
const ProposalWidget = lazyWidget(() =>
  import("./ProposalWidget").then((module) => ({ default: module.ProposalWidget }))
);
const IdeationWidget = lazyWidget(() =>
  import("./IdeationWidget").then((module) => ({ default: module.IdeationWidget }))
);
const ChildSessionWidget = lazyWidget(() =>
  import("./ChildSessionWidget").then((module) => ({ default: module.ChildSessionWidget }))
);
const GrepWidget = lazyWidget(() =>
  import("./GrepWidget").then((module) => ({ default: module.GrepWidget }))
);
const GlobWidget = lazyWidget(() =>
  import("./GlobWidget").then((module) => ({ default: module.GlobWidget }))
);
const ListDirWidget = lazyWidget(() =>
  import("./ListDirWidget").then((module) => ({ default: module.ListDirWidget }))
);
const ReadWidget = lazyWidget(() =>
  import("./ReadWidget").then((module) => ({ default: module.ReadWidget }))
);
const BashWidget = lazyWidget(() =>
  import("./BashWidget").then((module) => ({ default: module.BashWidget }))
);
const FileChangeWidget = lazyWidget(() =>
  import("./FileChangeWidget").then((module) => ({ default: module.FileChangeWidget }))
);
const SkillWidget = lazyWidget(() =>
  import("./SkillWidget").then((module) => ({ default: module.SkillWidget }))
);
const ProjectOrchestrationWidget = lazyWidget(() =>
  import("./ProjectOrchestrationWidget").then((module) => ({
    default: module.ProjectOrchestrationWidget,
  }))
);
const AgentTaskWidget = lazyWidget(() =>
  import("./AgentTaskWidget").then((module) => ({ default: module.AgentTaskWidget }))
);
const TaskCreateWidget = lazyWidget(() =>
  import("./TaskWidgets").then((module) => ({ default: module.TaskCreateWidget }))
);
const TaskUpdateWidget = lazyWidget(() =>
  import("./TaskWidgets").then((module) => ({ default: module.TaskUpdateWidget }))
);
const TaskListWidget = lazyWidget(() =>
  import("./TaskWidgets").then((module) => ({ default: module.TaskListWidget }))
);
const SessionContextWidget = lazyWidget(() =>
  import("./McpContextWidgets").then((module) => ({ default: module.SessionContextWidget }))
);
const SearchMemoriesWidget = lazyWidget(() =>
  import("./McpContextWidgets").then((module) => ({ default: module.SearchMemoriesWidget }))
);
const AgentWorkflowWidget = lazyWidget(() =>
  import("./AgentWorkflowWidget").then((module) => ({
    default: module.AgentWorkflowWidget,
  }))
);

/**
 * The widget registry. Maps tool names to specialized widget components.
 * Tool names should be lowercase to match normalized lookup in ToolCallIndicator.
 */
export const TOOL_CALL_WIDGETS: ToolCallWidgetRegistry = {
  // Bash tool → BashWidget (terminal output card)
  "bash": BashWidget,
  "file_change": FileChangeWidget,
  // File read tool → ReadWidget (file preview card)
  "read": ReadWidget,
  // Search tools → GrepWidget / GlobWidget
  grep: GrepWidget,
  glob: GlobWidget,
  list_dir: ListDirWidget,
  // Skill tool → SkillWidget (skill invocation card)
  "skill": SkillWidget,
  // Context tool → ContextWidget (always-visible context card)
  // Bare-name entries kept for backward compat with non-MCP contexts (test fixtures, CLI direct mode)
  "get_task_context": ContextWidget,
  // MCP-prefixed entries for actual MCP tool calls (getToolCallWidget uses exact-match lookup)
  "mcp__ralphx__get_task_context": ContextWidget,
  // Step lifecycle tools → StepIndicator (ultra-compact inline indicators)
  "mcp__ralphx__start_step": StepIndicator,
  "mcp__ralphx__complete_step": StepIndicator,
  "mcp__ralphx__add_step": StepIndicator,
  "mcp__ralphx__skip_step": StepIndicator,
  "mcp__ralphx__fail_step": StepIndicator,
  "mcp__ralphx__get_step_progress": StepIndicator,
  // Steps manifest → StepsManifestWidget (collapsible checklist)
  "get_task_steps": StepsManifestWidget,
  "mcp__ralphx__get_task_steps": StepsManifestWidget,
  // Issues summary → IssuesSummaryWidget (severity-badged issue list)
  "get_task_issues": IssuesSummaryWidget,
  "mcp__ralphx__get_task_issues": IssuesSummaryWidget,
  // Artifact tools → ArtifactWidget (type badge + title + markdown preview)
  "get_artifact": ArtifactWidget,
  "get_artifact_version": ArtifactWidget,
  "get_related_artifacts": ArtifactWidget,
  "search_project_artifacts": ArtifactWidget,
  "mcp__ralphx__get_artifact": ArtifactWidget,
  "mcp__ralphx__get_artifact_version": ArtifactWidget,
  "mcp__ralphx__get_related_artifacts": ArtifactWidget,
  "mcp__ralphx__search_project_artifacts": ArtifactWidget,
  // Review tools → ReviewWidget (outcome-colored cards + note list)
  "complete_review": ReviewWidget,
  "get_review_notes": ReviewWidget,
  "mcp__ralphx__complete_review": ReviewWidget,
  "mcp__ralphx__get_review_notes": ReviewWidget,
  // Merge tools → MergeWidget (success/conflict/incomplete cards + merge target)
  "mcp__ralphx__complete_merge": MergeWidget,
  "mcp__ralphx__report_conflict": MergeWidget,
  "mcp__ralphx__report_incomplete": MergeWidget,
  "mcp__ralphx__get_merge_target": MergeWidget,
  // Proposal CRUD tools → ProposalWidget
  "create_task_proposal": ProposalWidget,
  "update_task_proposal": ProposalWidget,
  "delete_task_proposal": ProposalWidget,
  "mcp__ralphx__create_task_proposal": ProposalWidget,
  "mcp__ralphx__update_task_proposal": ProposalWidget,
  "mcp__ralphx__delete_task_proposal": ProposalWidget,
  // Ideation session tools → IdeationWidget
  "mcp__ralphx__create_plan_artifact": IdeationWidget,
  "mcp__ralphx__update_plan_artifact": IdeationWidget,
  "mcp__ralphx__link_proposals_to_plan": IdeationWidget,
  "mcp__ralphx__ask_user_question": IdeationWidget,
  "mcp__ralphx__list_session_proposals": IdeationWidget,
  "mcp__ralphx__get_proposal": IdeationWidget,
  "mcp__ralphx__get_session_plan": IdeationWidget,
  "mcp__ralphx__analyze_session_dependencies": IdeationWidget,
  "mcp__ralphx__edit_plan_artifact": IdeationWidget,
  "mcp__ralphx__send_ideation_session_message": IdeationWidget,
  "mcp__ralphx__finalize_proposals": IdeationWidget,
  "mcp__ralphx__cross_project_guide": IdeationWidget,
  // Child session creation → ChildSessionWidget
  "mcp__ralphx__create_child_session": ChildSessionWidget,
  "mcp__ralphx__start_ideation_session": ChildSessionWidget,
  "mcp__ralphx__v1_start_ideation": ChildSessionWidget,
  "v1_start_ideation": ChildSessionWidget,
  // Project-agent external MCP orchestration checks -> quiet/no-op widgets after completion
  "mcp__ralphx__v1_get_agent_guide": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_list_ideation_sessions": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_get_project_status": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_get_ideation_status": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_get_ideation_messages": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_get_plan": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_get_plan_verification": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_list_proposals": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_get_session_tasks": ProjectOrchestrationWidget,
  "mcp__ralphx__v1_send_ideation_message": ProjectOrchestrationWidget,
  "v1_get_agent_guide": ProjectOrchestrationWidget,
  "v1_list_ideation_sessions": ProjectOrchestrationWidget,
  "v1_get_project_status": ProjectOrchestrationWidget,
  "v1_get_ideation_status": ProjectOrchestrationWidget,
  "v1_get_ideation_messages": ProjectOrchestrationWidget,
  "v1_get_plan": ProjectOrchestrationWidget,
  "v1_get_plan_verification": ProjectOrchestrationWidget,
  "v1_list_proposals": ProjectOrchestrationWidget,
  "v1_get_session_tasks": ProjectOrchestrationWidget,
  "v1_send_ideation_message": ProjectOrchestrationWidget,
  // Native agent task tools -> AgentTaskWidget
  "create_agent_task": AgentTaskWidget,
  "get_agent_task": AgentTaskWidget,
  "list_agent_tasks": AgentTaskWidget,
  "update_agent_task": AgentTaskWidget,
  "claim_agent_task": AgentTaskWidget,
  "complete_agent_task": AgentTaskWidget,
  "get_delegate_assignment": AgentTaskWidget,
  "complete_delegate_assignment": AgentTaskWidget,
  "release_delegate_assignment": AgentTaskWidget,
  // Task management tools → TaskWidgets
  "taskcreate": TaskCreateWidget,
  "taskupdate": TaskUpdateWidget,
  "tasklist": TaskListWidget,
  // MCP context/session/memory tools → McpContextWidgets
  "mcp__ralphx__get_parent_session_context": SessionContextWidget,
  "mcp__ralphx__search_memories": SearchMemoriesWidget,
  // Scripted Agent Workflow approval, progress, and lifecycle controls.
  create_agent_workflow_script: AgentWorkflowWidget,
  start_agent_workflow_run: AgentWorkflowWidget,
  get_agent_workflow_run: AgentWorkflowWidget,
  pause_agent_workflow_run: AgentWorkflowWidget,
  resume_agent_workflow_run: AgentWorkflowWidget,
  cancel_agent_workflow_run: AgentWorkflowWidget,
  "mcp__ralphx__create_agent_workflow_script": AgentWorkflowWidget,
  "mcp__ralphx__start_agent_workflow_run": AgentWorkflowWidget,
  "mcp__ralphx__get_agent_workflow_run": AgentWorkflowWidget,
  "mcp__ralphx__pause_agent_workflow_run": AgentWorkflowWidget,
  "mcp__ralphx__resume_agent_workflow_run": AgentWorkflowWidget,
  "mcp__ralphx__cancel_agent_workflow_run": AgentWorkflowWidget,
};

/**
 * Look up a specialized widget for a tool name.
 * Returns undefined if no specialized widget is registered.
 */
export function getToolCallWidget(toolName: string): ToolCallWidgetComponent | undefined {
  for (const candidate of getToolCallLookupCandidates(toolName)) {
    const widget = TOOL_CALL_WIDGETS[candidate];
    if (widget) {
      return widget;
    }
  }

  return undefined;
}
