import React from "react";
import {
  formatMessageAttributionTooltip,
  formatProviderHarnessLabel,
  formatProviderModelEffortLabel,
  getProviderHarnessBadgeStyle,
  type ProviderMetadata,
} from "./provider-harness";
import { getModelColor, getSubagentTypeColor } from "./tool-call-utils";
import {
  buildTaskCardSummaryParts,
  getTaskCardKindLabel,
  type TaskCardSummaryMetrics,
} from "./TaskCardShared.utils";

function TaskCardBadge({
  children,
  className = "",
  style,
  title,
}: {
  children: React.ReactNode;
  className?: string | undefined;
  style: React.CSSProperties;
  title?: string | undefined;
}) {
  return (
    <span
      className={`text-[0.625rem] px-1.5 py-0.5 rounded flex-shrink-0 ${className}`.trim()}
      style={style}
      title={title}
    >
      {children}
    </span>
  );
}

export function TaskCardKindBadge({ toolName }: { toolName: string }) {
  const kind = getTaskCardKindLabel(toolName);

  const style =
    kind === "Delegate"
      ? {
          backgroundColor: "color-mix(in srgb, var(--status-success) 12%, transparent)",
          color: "var(--status-success)",
        }
      : kind === "Agent"
        ? {
            backgroundColor: "color-mix(in srgb, var(--accent-primary) 12%, transparent)",
            color: "var(--accent-primary)",
          }
        : {
            backgroundColor: "color-mix(in srgb, var(--text-muted) 12%, transparent)",
            color: "var(--text-secondary)",
          };

  return (
    <TaskCardBadge className="font-medium" style={style}>
      {kind}
    </TaskCardBadge>
  );
}

export function TaskCardSubagentTypeBadge({
  subagentType,
}: {
  subagentType?: string | null | undefined;
}) {
  if (!subagentType || subagentType === "agent") {
    return null;
  }

  const color = getSubagentTypeColor(subagentType);
  return (
    <TaskCardBadge
      className="font-medium"
      style={{
        backgroundColor: color.bg,
        color: color.text,
      }}
    >
      {subagentType}
    </TaskCardBadge>
  );
}

export function TaskCardProviderHarnessBadge({
  providerHarness,
  providerMetadata,
}: {
  providerHarness?: string | null | undefined;
  providerMetadata: ProviderMetadata;
}) {
  const label = formatProviderHarnessLabel(providerHarness);
  if (!label) {
    return null;
  }

  return (
    <TaskCardBadge
      className="font-medium"
      style={getProviderHarnessBadgeStyle(providerHarness)}
      title={formatMessageAttributionTooltip(providerMetadata) ?? undefined}
    >
      {label}
    </TaskCardBadge>
  );
}

export function TaskCardModelBadge({
  label,
  colorKey,
  providerMetadata,
}: {
  label?: string | null | undefined;
  colorKey?: string | null | undefined;
  providerMetadata?: ProviderMetadata | undefined;
}) {
  if (!label) {
    return null;
  }

  const color = getModelColor(colorKey ?? label);
  const tooltip = providerMetadata
    ? formatMessageAttributionTooltip(providerMetadata) ?? undefined
    : undefined;

  return (
    <TaskCardBadge
      style={{
        backgroundColor: color.bg,
        color: color.text,
      }}
      title={tooltip}
    >
      {label}
    </TaskCardBadge>
  );
}

export function TaskCardProviderModelBadge({
  providerMetadata,
}: {
  providerMetadata: ProviderMetadata;
}) {
  const label = formatProviderModelEffortLabel(providerMetadata);
  const colorKey = providerMetadata.effectiveModelId ?? providerMetadata.logicalModel ?? label;
  return (
    <TaskCardModelBadge
      label={label}
      colorKey={colorKey}
      providerMetadata={providerMetadata}
    />
  );
}

export function TaskCardStatusBadge({
  label,
  tone = "warning",
}: {
  label?: string | null | undefined;
  tone?: "warning" | "error";
}) {
  if (!label) {
    return null;
  }

  const style =
    tone === "error"
      ? {
          backgroundColor: "color-mix(in srgb, var(--status-error) 18%, transparent)",
          color: "var(--status-error)",
        }
      : {
          backgroundColor: "color-mix(in srgb, var(--status-warning) 15%, transparent)",
          color: "var(--status-warning)",
        };

  return (
    <TaskCardBadge className="font-medium" style={style}>
      {label}
    </TaskCardBadge>
  );
}

export function TaskCardSummary({
  metrics,
  className = "",
}: {
  metrics: TaskCardSummaryMetrics;
  className?: string;
}) {
  const parts = buildTaskCardSummaryParts(metrics);
  if (parts.length === 0) {
    return null;
  }

  return (
    <span
      className={`text-xs ${className}`.trim()}
      style={{ color: "var(--text-muted)" }}
    >
      {parts.join(" \u00B7 ")}
    </span>
  );
}

export function TaskCardRuntimeDetails({
  providerMetadata,
}: {
  providerMetadata: ProviderMetadata;
}) {
  const rows: Array<[string, string | null | undefined]> = [
    ["Harness", formatProviderHarnessLabel(providerMetadata.providerHarness)],
    ["Upstream provider", providerMetadata.upstreamProvider],
    ["Provider profile", providerMetadata.providerProfile],
    ["Logical model", providerMetadata.logicalModel],
    ["Effective model", providerMetadata.effectiveModelId],
    ["Logical effort", providerMetadata.logicalEffort],
    ["Effective effort", providerMetadata.effectiveEffort],
    ["Approval policy", providerMetadata.approvalPolicy],
    ["Sandbox mode", providerMetadata.sandboxMode],
    ["Provider session", providerMetadata.providerSessionId],
    [
      "Input tokens",
      providerMetadata.inputTokens?.toLocaleString("en-US"),
    ],
    [
      "Output tokens",
      providerMetadata.outputTokens?.toLocaleString("en-US"),
    ],
    [
      "Cache tokens",
      providerMetadata.cacheCreationTokens != null
        || providerMetadata.cacheReadTokens != null
        ? ((providerMetadata.cacheCreationTokens ?? 0)
          + (providerMetadata.cacheReadTokens ?? 0)).toLocaleString("en-US")
        : null,
    ],
    [
      "Estimated cost",
      providerMetadata.estimatedUsd != null
        ? `$${providerMetadata.estimatedUsd.toFixed(2)}`
        : null,
    ],
  ].filter((row): row is [string, string] => typeof row[1] === "string" && row[1].length > 0);

  if (rows.length === 0) return null;

  return (
    <dl
      className="grid grid-cols-[max-content,minmax(0,1fr)] gap-x-3 gap-y-1 text-[0.6875rem] mb-3"
      data-testid="delegate-runtime-details"
    >
      {rows.map(([label, value]) => (
        <React.Fragment key={label}>
          <dt style={{ color: "var(--text-muted)" }}>{label}</dt>
          <dd className="truncate" style={{ color: "var(--text-secondary)" }}>{value}</dd>
        </React.Fragment>
      ))}
    </dl>
  );
}
