/**
 * IdeationWidget — Compact indicators for ideation session tools
 *
 * Handles:
 * - create_plan_artifact: plan title + "Plan created"
 * - update_plan_artifact: "Plan updated"
 * - link_proposals_to_plan: count of linked proposals
 * - ask_user_question: question text preview
 * - list_session_proposals: proposal count
 * - get_proposal: summary card
 * - get_session_plan: summary card
 * - analyze_session_dependencies: summary card
 * - finalize_proposals: task count + deps + session status
 * - cross_project_guide: cross-project detection + gate status
 */

import React from "react";
import {
  FileText,
  Link,
  MessageCircleQuestion,
  List,
  Search,
  GitBranch,
  MessageSquare,
  CheckCircle2,
  FolderTree,
} from "lucide-react";
import { InlineIndicator, Badge, WidgetRow } from "./shared";
import { colors, getString, getNumber, getArray, getBool, parseMcpToolResult } from "./shared.constants";
import type { ToolCallWidgetProps } from "./shared.constants";

// ============================================================================
// Helpers
// ============================================================================

type IdeationTool =
  | "create_plan_artifact"
  | "update_plan_artifact"
  | "edit_plan_artifact"
  | "send_ideation_session_message"
  | "link_proposals_to_plan"
  | "ask_user_question"
  | "list_session_proposals"
  | "get_proposal"
  | "get_session_plan"
  | "analyze_session_dependencies"
  | "finalize_proposals"
  | "cross_project_guide";

function getToolType(toolName: string): IdeationTool | null {
  const name = toolName.toLowerCase();
  if (name.includes("create_plan_artifact")) return "create_plan_artifact";
  if (name.includes("update_plan_artifact")) return "update_plan_artifact";
  if (name.includes("edit_plan_artifact")) return "edit_plan_artifact";
  if (name.includes("send_ideation_session_message")) return "send_ideation_session_message";
  if (name.includes("link_proposals_to_plan")) return "link_proposals_to_plan";
  if (name.includes("ask_user_question")) return "ask_user_question";
  if (name.includes("list_session_proposals")) return "list_session_proposals";
  if (name.includes("get_proposal")) return "get_proposal";
  if (name.includes("get_session_plan")) return "get_session_plan";
  if (name.includes("analyze_session_dependencies")) return "analyze_session_dependencies";
  if (name.includes("finalize_proposals")) return "finalize_proposals";
  if (name.includes("cross_project_guide")) return "cross_project_guide";
  return null;
}

// ============================================================================
// Sub-renderers
// ============================================================================

function PlanCreated({ toolCall, compact }: ToolCallWidgetProps) {
  const parsed = parseMcpToolResult(toolCall.result);
  const title = getString(parsed, "name")
    ?? getString(toolCall.arguments, "title")
    ?? "Plan";
  const version = getNumber(parsed, "version");

  return (
    <WidgetRow compact={compact}>
      <FileText size={12} style={{ color: colors.accent, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {title}
      </span>
      {version != null && <Badge variant="muted" compact>v{version}</Badge>}
      <Badge variant="success" compact>Plan created</Badge>
    </WidgetRow>
  );
}

function PlanUpdated({ toolCall, compact }: ToolCallWidgetProps) {
  const parsed = parseMcpToolResult(toolCall.result);
  const name = getString(parsed, "name");
  const version = getNumber(parsed, "version");

  if (!name) {
    return <InlineIndicator icon={<FileText size={11} style={{ color: colors.blue }} />} text="Plan updated" />;
  }

  return (
    <WidgetRow compact={compact}>
      <FileText size={11} style={{ color: colors.blue, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {name}
      </span>
      {version != null && <Badge variant="muted" compact>v{version}</Badge>}
      <Badge variant="blue" compact>Updated</Badge>
    </WidgetRow>
  );
}

function PlanEdited({ toolCall, compact }: ToolCallWidgetProps) {
  const parsed = parseMcpToolResult(toolCall.result);
  const name = getString(parsed, "name");
  const version = getNumber(parsed, "version");
  const edits = getArray(toolCall.arguments, "edits");
  const count = edits?.length ?? 0;

  if (!name) {
    return <InlineIndicator icon={<FileText size={11} style={{ color: colors.blue }} />} text="Plan edited" />;
  }

  return (
    <WidgetRow compact={compact}>
      <FileText size={11} style={{ color: colors.blue, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {name}
      </span>
      {version != null && <Badge variant="muted" compact>v{version}</Badge>}
      {!compact && count > 0 && <Badge variant="muted" compact>{count} edits</Badge>}
      <Badge variant="blue" compact>Edited</Badge>
    </WidgetRow>
  );
}

function SessionMessage({ toolCall, compact }: ToolCallWidgetProps) {
  const message = getString(toolCall.arguments, "message");
  const parsed = parseMcpToolResult(toolCall.result);
  const deliveryStatus = getString(parsed, "delivery_status");

  if (!deliveryStatus) {
    return <InlineIndicator icon={<MessageSquare size={11} style={{ color: colors.textMuted }} />} text="Sending message..." />;
  }

  const maxLen = compact ? 60 : 80;
  const preview = message ?? "";
  const truncated = preview.length > maxLen ? preview.slice(0, maxLen) + "..." : preview;

  const statusVariant =
    deliveryStatus === "sent" ? "success" :
    deliveryStatus === "queued" ? "blue" :
    "accent";

  return (
    <WidgetRow compact={compact}>
      <MessageSquare size={11} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {truncated}
      </span>
      <Badge variant={statusVariant} compact>{deliveryStatus}</Badge>
    </WidgetRow>
  );
}

function LinkProposals({ toolCall, compact }: ToolCallWidgetProps) {
  // Count from args (proposal_ids array)
  const proposalIds = getArray(toolCall.arguments, "proposal_ids");
  const count = proposalIds?.length;

  const text = count != null
    ? `${count} proposal${count !== 1 ? "s" : ""} linked to plan`
    : "Proposals linked to plan";

  return (
    <WidgetRow compact={compact}>
      <Link size={11} style={{ color: colors.blue, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
        }}
      >
        {text}
      </span>
      {count != null && <Badge variant="blue" compact>{count}</Badge>}
    </WidgetRow>
  );
}

function AskUserQuestion({ toolCall, compact }: ToolCallWidgetProps) {
  const question = getString(toolCall.arguments, "question");
  const header = getString(toolCall.arguments, "header");
  const preview = header ?? question ?? "Asking user...";

  // Truncate long questions
  const maxLen = compact ? 60 : 80;
  const truncated = preview.length > maxLen
    ? preview.slice(0, maxLen) + "..."
    : preview;

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 7,
        padding: compact ? "3px 10px" : "5px 10px",
        margin: "2px 0",
        height: compact ? undefined : 26,
        boxSizing: "border-box",
      }}
    >
      <MessageCircleQuestion size={12} style={{ color: colors.accent, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
          fontStyle: "italic",
        }}
      >
        {truncated}
      </span>
      <Badge variant="accent" compact>Question</Badge>
    </div>
  );
}

function ListProposals({ toolCall, compact }: ToolCallWidgetProps) {
  const count = getNumber(toolCall.result, "count");
  const proposals = getArray(toolCall.result, "proposals");
  const n = count ?? proposals?.length;

  if (n == null) {
    return <InlineIndicator icon={<List size={11} style={{ color: colors.textMuted }} />} text="Loading proposals..." />;
  }

  return (
    <WidgetRow compact={compact}>
      <List size={12} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
        }}
      >
        {n} proposal{n !== 1 ? "s" : ""} in session
      </span>
      <Badge variant="muted" compact>{n}</Badge>
    </WidgetRow>
  );
}

function GetProposal({ toolCall, compact }: ToolCallWidgetProps) {
  const parsed = parseMcpToolResult(toolCall.result);
  const title = getString(parsed, "title");
  const category = getString(parsed, "category");

  if (!title) {
    return <InlineIndicator icon={<Search size={11} style={{ color: colors.textMuted }} />} text="Loading proposal..." />;
  }

  return (
    <WidgetRow compact={compact}>
      <Search size={11} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {title}
      </span>
      {category && <Badge variant="accent" compact>{category}</Badge>}
      <Badge variant="muted" compact>Loaded</Badge>
    </WidgetRow>
  );
}

function GetSessionPlan({ toolCall, compact }: ToolCallWidgetProps) {
  // Result is the artifact or null
  const parsed = parseMcpToolResult(toolCall.result);
  const name = getString(parsed, "name");
  const version = getNumber(parsed, "version");

  if (!name) {
    return <InlineIndicator icon={<FileText size={11} style={{ color: colors.textMuted }} />} text="No plan artifact" />;
  }

  return (
    <WidgetRow compact={compact}>
      <FileText size={11} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {name}
      </span>
      {version != null && <Badge variant="muted" compact>v{version}</Badge>}
      <Badge variant="muted" compact>Loaded</Badge>
    </WidgetRow>
  );
}

function AnalyzeDependencies({ toolCall, compact }: ToolCallWidgetProps) {
  const result = toolCall.result;
  const totalProposals = getNumber(result, "total_proposals")
    ?? (result != null && typeof result === "object" && "summary" in (result as Record<string, unknown>)
      ? getNumber((result as Record<string, unknown>).summary, "total_proposals")
      : undefined);
  const hasCycles = result != null && typeof result === "object"
    && (result as Record<string, unknown>).has_cycles === true;

  if (totalProposals == null) {
    return <InlineIndicator icon={<GitBranch size={11} style={{ color: colors.textMuted }} />} text="Analyzing dependencies..." />;
  }

  return (
    <WidgetRow compact={compact}>
      <GitBranch size={11} style={{ color: hasCycles ? colors.error : colors.success, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
        }}
      >
        {totalProposals} proposals analyzed
      </span>
      {hasCycles && <Badge variant="error" compact>Cycles</Badge>}
      {!hasCycles && <Badge variant="success" compact>OK</Badge>}
    </WidgetRow>
  );
}

function FinalizeProposals({ toolCall, compact }: ToolCallWidgetProps) {
  const parsed = parseMcpToolResult(toolCall.result);
  const taskIds = getArray(parsed, "created_task_ids");
  const depsCreated = getNumber(parsed, "dependencies_created");
  const sessionStatus = getString(parsed, "session_status");
  const warnings = getArray(parsed, "warnings");

  if (taskIds == null && sessionStatus == null) {
    return <InlineIndicator icon={<CheckCircle2 size={11} style={{ color: colors.success }} />} text="Finalizing proposals..." />;
  }

  const taskCount = taskIds?.length ?? 0;
  const statusVariant = sessionStatus?.toLowerCase() === "accepted" ? "success" : "muted";
  const statusLabel = sessionStatus
    ? sessionStatus.charAt(0).toUpperCase() + sessionStatus.slice(1)
    : null;

  return (
    <WidgetRow compact={compact}>
      <CheckCircle2 size={11} style={{ color: colors.success, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
        }}
      >
        {taskCount === 0 ? "No tasks created" : `${taskCount} task${taskCount !== 1 ? "s" : ""} created`}
      </span>
      {depsCreated != null && depsCreated > 0 && (
        <Badge variant="muted" compact>{depsCreated} deps</Badge>
      )}
      {statusLabel && <Badge variant={statusVariant} compact>{statusLabel}</Badge>}
      {warnings != null && warnings.length > 0 && (
        <Badge variant="accent" compact>{warnings.length} warnings</Badge>
      )}
    </WidgetRow>
  );
}

function CrossProjectGuide({ toolCall, compact }: ToolCallWidgetProps) {
  const parsed = parseMcpToolResult(toolCall.result);
  const hasCrossPaths = getBool(parsed, "has_cross_project_paths");
  const detectedPaths = getArray(parsed, "detected_paths");
  const gateStatus = getString(parsed, "gate_status");

  if (hasCrossPaths == null && gateStatus == null) {
    return <InlineIndicator icon={<FolderTree size={11} style={{ color: colors.textMuted }} />} text="Analyzing cross-project paths..." />;
  }

  const pathCount = detectedPaths?.length ?? 0;
  const gateVariant =
    gateStatus === "set" ? "success" :
    gateStatus === "backend_unavailable" ? "error" :
    "muted";
  const gateLabel =
    gateStatus === "set" ? "Gate set" :
    gateStatus === "backend_unavailable" ? "Gate error" :
    gateStatus === "no_session_id" ? "No gate" :
    gateStatus ?? null;

  return (
    <WidgetRow compact={compact}>
      <FolderTree size={11} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span
        style={{
          flex: 1,
          fontSize: compact ? 10.5 : 11,
          color: colors.textSecondary,
        }}
      >
        {hasCrossPaths ? `${pathCount} project${pathCount !== 1 ? "s" : ""} detected` : "No cross-project paths"}
      </span>
      {hasCrossPaths
        ? <Badge variant="success" compact>Cross-project</Badge>
        : <Badge variant="muted" compact>Single project</Badge>
      }
      {gateLabel && <Badge variant={gateVariant} compact>{gateLabel}</Badge>}
    </WidgetRow>
  );
}

// ============================================================================
// IdeationWidget (main component)
// ============================================================================

export const IdeationWidget = React.memo(function IdeationWidget(props: ToolCallWidgetProps) {
  const toolType = getToolType(props.toolCall.name);

  switch (toolType) {
    case "create_plan_artifact":
      return <div data-testid="ideation-widget-create-plan"><PlanCreated {...props} /></div>;
    case "update_plan_artifact":
      return <div data-testid="ideation-widget-update-plan"><PlanUpdated {...props} /></div>;
    case "edit_plan_artifact":
      return <div data-testid="ideation-widget-edit-plan"><PlanEdited {...props} /></div>;
    case "send_ideation_session_message":
      return <div data-testid="ideation-widget-session-message"><SessionMessage {...props} /></div>;
    case "link_proposals_to_plan":
      return <div data-testid="ideation-widget-link-proposals"><LinkProposals {...props} /></div>;
    case "ask_user_question":
      return <div data-testid="ideation-widget-ask-question" style={props.compact ? undefined : { display: "flow-root" }}><AskUserQuestion {...props} /></div>;
    case "list_session_proposals":
      return <div data-testid="ideation-widget-list-proposals"><ListProposals {...props} /></div>;
    case "get_proposal":
      return <div data-testid="ideation-widget-get-proposal"><GetProposal {...props} /></div>;
    case "get_session_plan":
      return <div data-testid="ideation-widget-get-session-plan"><GetSessionPlan {...props} /></div>;
    case "analyze_session_dependencies":
      return <div data-testid="ideation-widget-analyze-deps"><AnalyzeDependencies {...props} /></div>;
    case "finalize_proposals":
      return <div data-testid="ideation-widget-finalize"><FinalizeProposals {...props} /></div>;
    case "cross_project_guide":
      return <div data-testid="ideation-widget-cross-project"><CrossProjectGuide {...props} /></div>;
    default:
      return <div data-testid="ideation-widget-fallback"><InlineIndicator text={props.toolCall.name} /></div>;
  }
});
