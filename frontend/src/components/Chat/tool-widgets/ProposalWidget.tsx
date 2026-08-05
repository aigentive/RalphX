/**
 * ProposalWidget — Compact card for proposal CRUD tool calls
 *
 * Handles: create_task_proposal, update_task_proposal, archive_task_proposal
 *
 * Design:
 * - create: title + category badge + "Created" indicator
 * - update: title + "Updated" badge with changed fields summary
 * - archive: "Archived" badge with proposal title
 */

import React from "react";
import { Plus, Pencil, Trash2 } from "lucide-react";
import { InlineIndicator, Badge, WidgetRow } from "./shared";
import { colors, getString, parseMcpToolResult } from "./shared.constants";
import type { ToolCallWidgetProps } from "./shared.constants";

// ============================================================================
// Helpers
// ============================================================================

type ProposalAction = "created" | "updated" | "archived";

function getAction(toolName: string): ProposalAction {
  const name = toolName.toLowerCase();
  if (name.includes("archive")) return "archived";
  if (name.includes("update")) return "updated";
  return "created";
}

/** Extract proposal title from args or result */
function extractTitle(toolCall: ToolCallWidgetProps["toolCall"]): string {
  // Try result first (has the canonical title after create/update)
  const fromResult = getString(parseMcpToolResult(toolCall.result), "title");
  if (fromResult) return fromResult;

  // Args for create/update
  const fromArgs = getString(toolCall.arguments, "title");
  if (fromArgs) return fromArgs;

  return "Proposal";
}

/** Extract category from args or result */
function extractCategory(toolCall: ToolCallWidgetProps["toolCall"]): string | undefined {
  return getString(parseMcpToolResult(toolCall.result), "category") ?? getString(toolCall.arguments, "category");
}

/** For update: build summary of which fields changed with richer display */
function extractChangedFields(toolCall: ToolCallWidgetProps["toolCall"]): string[] {
  const args = toolCall.arguments;
  if (args == null || typeof args !== "object") return [];

  const fields: string[] = [];
  const a = args as Record<string, unknown>;
  if (a.title != null) fields.push("title");
  if (a.description != null) fields.push("description");
  if (a.category != null) fields.push(`category → ${String(a.category)}`);
  if (a.user_priority != null) fields.push(`priority → ${String(a.user_priority)}`);
  if (a.steps != null) {
    const count = Array.isArray(a.steps) ? a.steps.length : 0;
    fields.push(`steps (${count})`);
  }
  if (a.acceptance_criteria != null) {
    const count = Array.isArray(a.acceptance_criteria) ? a.acceptance_criteria.length : 0;
    fields.push(`criteria (${count})`);
  }
  return fields;
}

// ============================================================================
// Action config
// ============================================================================

const actionConfig: Record<ProposalAction, {
  icon: React.ReactNode;
  label: string;
  badgeVariant: "success" | "blue" | "error";
  color: string;
}> = {
  created: {
    icon: <Plus size={12} />,
    label: "Created",
    badgeVariant: "success",
    color: colors.success,
  },
  updated: {
    icon: <Pencil size={11} />,
    label: "Updated",
    badgeVariant: "blue",
    color: colors.blue,
  },
  archived: {
    icon: <Trash2 size={11} />,
    label: "Archived",
    badgeVariant: "error",
    color: colors.error,
  },
};

// ============================================================================
// ProposalWidget
// ============================================================================

export const ProposalWidget = React.memo(function ProposalWidget({
  toolCall,
  compact = false,
}: ToolCallWidgetProps) {
  const action = getAction(toolCall.name);
  const config = actionConfig[action];
  const title = extractTitle(toolCall);
  const category = extractCategory(toolCall);
  const changedFields = action === "updated" ? extractChangedFields(toolCall) : [];

  // For archive with no title found, show a minimal indicator
  if (action === "archived" && title === "Proposal") {
    return (
      <InlineIndicator
        icon={<Trash2 size={11} style={{ color: colors.error }} />}
        text="Proposal archived"
      />
    );
  }

  return (
    <div data-testid={`proposal-widget-${action}`}>
      <WidgetRow compact={compact}>
        {/* Action icon */}
        <span
          style={{
            width: 14,
            height: 14,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: config.color,
          }}
        >
          {config.icon}
        </span>

        {/* Title (truncated) */}
        <span
          style={{
            flex: 1,
            fontSize: compact ? 10.5 : 11,
            color: action === "archived" ? colors.textMuted : colors.textSecondary,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            textDecoration: action === "archived" ? "line-through" : undefined,
          }}
        >
          {title}
        </span>

        {/* Category chip (only for create/update) */}
        {category && action !== "archived" && (
          <Badge variant="accent" compact>{category}</Badge>
        )}

        {/* Changed fields summary (only for update) */}
        {changedFields.length > 0 && (
          <span
            style={{
              fontSize: 10,
              color: colors.textMuted,
              flexShrink: 0,
              whiteSpace: "nowrap",
            }}
          >
            {changedFields.join(", ")}
          </span>
        )}

        {/* Action badge */}
        <Badge variant={config.badgeVariant} compact>{config.label}</Badge>
      </WidgetRow>
    </div>
  );
});
