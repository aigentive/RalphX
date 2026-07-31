import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { agentRunsApi } from "@/api/agent-runs";
import type { AgentRunAttribution } from "@/api/agent-runs";
import { isFeedbackRole, roleVerb, workedDurationLabel } from "./run-attribution";

type RunAttributionWidgetProps = {
  runId: string;
  startedAt: string;
  completedAt?: string | null;
  launchRole?: string | null;
};

function valueTransition(logical: string | null, effective: string | null) {
  if (!logical) return effective;
  if (!effective || logical === effective) return logical;
  return <>{logical}<span className="px-1" style={{ color: "var(--text-subtle)" }}>→</span>{effective}</>;
}

function formatCost(value: number | null) {
  return value == null ? null : `$${value.toFixed(4)}`;
}

function RoleChip({ launchRole }: { launchRole: string | null | undefined }) {
  if (!isFeedbackRole(launchRole)) return null;
  const reviewer = launchRole === "workspace_reviewer";
  return <span className="inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[0.625rem] font-semibold"
    style={{ color: reviewer ? "var(--status-info)" : "var(--status-warning)", backgroundColor: reviewer ? "var(--status-info-muted)" : "var(--status-warning-muted)", borderColor: reviewer ? "var(--status-info-border)" : "var(--status-warning-border)" }}>
    <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: reviewer ? "var(--status-info)" : "var(--status-warning)" }} />
    {roleVerb(launchRole)}
  </span>;
}

function AttributionPanel({ attribution }: { attribution: AgentRunAttribution | null }) {
  if (!attribution) return <div className="space-y-2 p-3" data-testid="run-attribution-loading">
    <div className="h-3 w-3/5 animate-pulse rounded bg-[var(--bg-hover)]" />
    <div className="h-3 w-4/5 animate-pulse rounded bg-[var(--bg-hover)]" />
  </div>;
  const duration = workedDurationLabel(attribution.startedAt, attribution.completedAt);
  const tokens = [
    attribution.inputTokens != null ? `in ${attribution.inputTokens}` : null,
    attribution.outputTokens != null ? `out ${attribution.outputTokens}` : null,
    attribution.cacheCreationTokens != null ? `cache write ${attribution.cacheCreationTokens}` : null,
    attribution.cacheReadTokens != null ? `cache read ${attribution.cacheReadTokens}` : null,
  ].filter(Boolean).join(" · ");
  const rows: Array<[string, React.ReactNode, boolean?]> = [
    ["Agent", attribution.agentName], ["Role", attribution.launchRole], ["Trigger", attribution.actionKind],
    ["Runtime source", attribution.runtimeSource], ["Provider", attribution.upstreamProvider ?? attribution.harness],
    ["Model", valueTransition(attribution.logicalModel, attribution.effectiveModelId)],
    ["Effort", valueTransition(attribution.logicalEffort, attribution.effectiveEffort)], ["Speed", attribution.serviceTier],
    ["Persona", attribution.personaSlug], ["Approval", attribution.approvalPolicy], ["Sandbox", attribution.sandboxMode],
    ["Session", attribution.providerSessionId, true], ["Run / chain", attribution.runChainId ? `${attribution.id} / ${attribution.runChainId}` : attribution.id, true],
    ["Tokens", tokens], ["Est. cost", formatCost(attribution.estimatedUsd)],
    ["Timing", `${attribution.startedAt}${attribution.completedAt ? ` → ${attribution.completedAt}` : ""} · ${duration}`],
  ];
  return <div className="ml-1 mt-1.5 w-full max-w-[430px] overflow-hidden rounded-[10px] border"
    data-testid="run-attribution-panel" style={{ borderColor: "var(--border-subtle)", backgroundColor: "var(--bg-surface)" }}>
    <div className="flex items-center gap-2 border-b px-3 py-2 text-[0.625rem] font-semibold uppercase tracking-[0.07em]"
      style={{ borderColor: "var(--border-subtle)", color: "var(--text-muted)" }}>Run attribution
      {attribution.harness ? <span className="ml-auto rounded-full border px-2 py-0.5 normal-case tracking-normal" style={{ color: "var(--accent-primary)", backgroundColor: "var(--accent-muted)", borderColor: "var(--accent-border)" }}>{attribution.harness}</span> : null}
    </div>
    <div className="space-y-0.5 p-1">
      {rows.filter(([, value]) => value != null && value !== "").map(([label, value, mono]) => <div key={label} className="flex items-baseline gap-3 rounded-md px-2 py-1.5 text-[0.6875rem] hover:bg-[var(--overlay-weak)]">
        <span className="w-[108px] shrink-0 font-medium" style={{ color: "var(--text-muted)" }}>{label}</span>
        <span className={`min-w-0 flex-1 break-words ${mono ? "font-mono text-[0.625rem]" : ""}`} style={{ color: mono ? "var(--text-secondary)" : "var(--text-primary)" }}>{value}</span>
      </div>)}
    </div>
  </div>;
}

export function RunAttributionWidget({ runId, startedAt, completedAt = null, launchRole = null }: RunAttributionWidgetProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [attribution, setAttribution] = useState<AgentRunAttribution | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!isExpanded || attribution || error) return;
    let active = true;
    void agentRunsApi.getAttribution(runId).then((result) => {
      if (active) setAttribution(result);
    }).catch(() => {
      if (active) setError("Run attribution is unavailable.");
    });
    return () => { active = false; };
  }, [attribution, error, isExpanded, runId]);
  const role = attribution?.launchRole ?? launchRole;
  const exactCompletedAt = attribution?.completedAt ?? completedAt;
  return <div className="mt-1.5" data-testid="run-attribution-widget">
    <button type="button" data-testid="run-attribution-toggle" data-chat-run-attribution-key={runId}
      aria-expanded={isExpanded} aria-label={`${roleVerb(role)} worked for ${workedDurationLabel(attribution?.startedAt ?? startedAt, exactCompletedAt)} ${isExpanded ? "Collapse" : "Expand"} run attribution.`}
      onClick={() => setIsExpanded((current) => !current)}
      className="inline-flex max-w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-[0.6875rem] font-medium transition-opacity hover:opacity-80"
      style={{ backgroundColor: "var(--bg-elevated)", color: "var(--text-secondary)" }}>
      {isExpanded ? <ChevronDown className="h-3 w-3 shrink-0" aria-hidden="true" /> : <ChevronRight className="h-3 w-3 shrink-0" aria-hidden="true" />}
      <span>{roleVerb(role)} worked for <span className="tabular-nums">{workedDurationLabel(attribution?.startedAt ?? startedAt, exactCompletedAt)}</span></span>
      <RoleChip launchRole={role} />
    </button>
    {isExpanded ? (error ? <p className="ml-1 mt-1 text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>{error}</p> : <AttributionPanel attribution={attribution} />) : null}
  </div>;
}
