import { useEffect, useState, type CSSProperties } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cpu,
  Info,
  TriangleAlert,
} from "lucide-react";

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ErrorBanner, SectionCard } from "./SettingsView.shared";
import {
  EXECUTION_LANES,
  type Harness,
  type AgentHarnessLaneView,
  type AgentLane,
  type KnownHarness,
  IDEATION_LANES,
} from "@/api/ideation-harness";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useAgentHarnessSettings } from "@/hooks/useIdeationHarnessSettings";
import {
  agentEffortOptionsForModel,
  agentModelOptionsForProvider,
  defaultEffortForModel,
  defaultModelForProvider,
  type AgentModelRegistry,
  type AgentProvider,
} from "@/lib/agent-models";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import {
  loadHarnessExpanded,
  loadHarnessTab,
  saveHarnessExpanded,
  saveHarnessExpandedBulk,
  saveHarnessTab,
  type HarnessTabValue,
} from "./settings-ui-state";

// ============================================================================
// Preset definitions
// ============================================================================

interface ModelPreset {
  value: string;
  display: string;
  description?: string;
}

const CLAUDE_MODEL_PRESETS = [
  { value: "sonnet", display: "sonnet" },
  { value: "opus", display: "opus" },
  { value: "haiku", display: "haiku" },
] as const satisfies readonly ModelPreset[];

const SELECT_TRIGGER_CLASS = "h-9 items-center";

// ============================================================================
// InlineNotice — soft alert card used for status and locked-policy notes
// ============================================================================

type NoticeTone = "ok" | "warn" | "info";

interface InlineNoticeProps {
  tone: NoticeTone;
  title?: React.ReactNode;
  children: React.ReactNode;
}

const NOTICE_STYLES: Record<
  NoticeTone,
  { style: CSSProperties; icon: React.ReactNode }
> = {
  ok: {
    style: {
      backgroundColor: "var(--notice-ok-bg)",
      borderColor: "var(--notice-ok-border)",
      color: "var(--notice-ok-text)",
    },
    icon: (
      <CheckCircle2
        className="w-3.5 h-3.5 shrink-0 mt-0.5"
        style={{ color: "var(--notice-ok-icon)" }}
      />
    ),
  },
  warn: {
    style: {
      backgroundColor: "var(--notice-warn-bg)",
      borderColor: "var(--notice-warn-border)",
      color: "var(--notice-warn-text)",
    },
    icon: (
      <TriangleAlert
        className="w-3.5 h-3.5 shrink-0 mt-0.5"
        style={{ color: "var(--notice-warn-icon)" }}
      />
    ),
  },
  info: {
    style: {
      backgroundColor: "var(--notice-info-bg)",
      borderColor: "var(--notice-info-border)",
      color: "var(--notice-info-text)",
    },
    icon: (
      <Info
        className="w-3.5 h-3.5 shrink-0 mt-0.5"
        style={{ color: "var(--notice-info-icon)" }}
      />
    ),
  },
};

function InlineNotice({ tone, title, children }: InlineNoticeProps) {
  const { style, icon } = NOTICE_STYLES[tone];
  return (
    <div
      className="settings-inline-notice flex items-start gap-2 rounded-md border px-3 py-2 text-[0.6875rem] leading-relaxed"
      style={style}
    >
      {icon}
      <div className="min-w-0 flex-1">
        {title && (
          <div
            className="font-medium"
            style={{ color: "var(--notice-title-text, var(--text-primary))" }}
          >
            {title}
          </div>
        )}
        <div className={title ? "mt-0.5" : undefined}>{children}</div>
      </div>
    </div>
  );
}

function isAgentProvider(value: string): value is AgentProvider {
  return value === "claude" || value === "codex";
}

function getModelPresets(
  harness: string,
  modelRegistry: AgentModelRegistry,
): readonly ModelPreset[] {
  if (!isAgentProvider(harness)) {
    return CLAUDE_MODEL_PRESETS;
  }
  return agentModelOptionsForProvider(harness, modelRegistry).map(
    ({ id, menuLabel, description }) => ({
      value: id,
      display: menuLabel,
      ...(description ? { description } : {}),
    })
  );
}

function getEffortOptions(
  isGlobal: boolean,
  harness: string,
  model: string | null,
  modelRegistry: AgentModelRegistry,
) {
  const provider = isAgentProvider(harness) ? harness : "claude";
  const modelId = model ?? defaultModelForProvider(provider, modelRegistry);
  return [
    {
      value: "inherit",
      label: "Default",
      description: isGlobal
        ? "Uses the app default from config"
        : "Uses the global default for this lane",
    },
    ...agentEffortOptionsForModel(provider, modelId, modelRegistry).map(
      ({ id, label, description }) => ({
        value: id,
        label,
        description,
      })
    ),
  ];
}

function effortLabel(value: string | null | undefined): string {
  if (!value || value === "inherit") return "Default";
  switch (value) {
    case "low": return "Low";
    case "medium": return "Medium";
    case "high": return "High";
    case "xhigh": return "Extra High";
    case "max": return "Max";
    default: return value;
  }
}

// ============================================================================
// ModelSelect — preset selector with safe fallback for existing custom values
// ============================================================================

const MODEL_DEFAULT_VALUE = "__default__";
const MODEL_CUSTOM_VALUE_PREFIX = "__custom__:";

interface ModelSelectProps {
  value: string | null;
  harness: string;
  disabled: boolean;
  onChange: (value: string | null) => void;
  laneLabel: string;
  isGlobal: boolean;
  testId: string;
  modelRegistry: AgentModelRegistry;
}

function modelSelectValue(
  value: string | null,
  presets: readonly ModelPreset[],
): string {
  if (!value) {
    return MODEL_DEFAULT_VALUE;
  }

  return presets.some((preset) => preset.value === value)
    ? value
    : `${MODEL_CUSTOM_VALUE_PREFIX}${value}`;
}

function ModelSelect({
  value,
  harness,
  disabled,
  onChange,
  laneLabel,
  isGlobal,
  testId,
  modelRegistry,
}: ModelSelectProps) {
  const presets = getModelPresets(harness, modelRegistry);
  const currentValue = modelSelectValue(value, presets);
  const defaultLabel = isGlobal ? "Harness default" : "Use global default";
  const defaultDescription = isGlobal
    ? "Follow this provider's built-in default model for the lane."
    : "Inherit the global model configured for this lane.";
  const hasCustomValue =
    value != null && !presets.some((preset) => preset.value === value);

  const triggerLabel = (() => {
    if (!value) return defaultLabel;
    const preset = presets.find((p) => p.value === value);
    return preset ? preset.display : value;
  })();

  return (
    <Select
      value={currentValue}
      onValueChange={(nextValue) => {
        const resolvedValue =
          nextValue === MODEL_DEFAULT_VALUE
            ? null
            : nextValue.startsWith(MODEL_CUSTOM_VALUE_PREFIX)
              ? nextValue.slice(MODEL_CUSTOM_VALUE_PREFIX.length)
              : nextValue;
        if (resolvedValue !== value) {
          onChange(resolvedValue);
        }
      }}
      disabled={disabled}
    >
      <SelectTrigger
        data-testid={testId}
        aria-label={`${laneLabel} model`}
        disabled={disabled}
        className={`${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)]`}
      >
        <SelectValue
          placeholder={harness === "codex" ? "Select Codex model" : "Select Claude model"}
        >
          <span className="truncate">{triggerLabel}</span>
        </SelectValue>
      </SelectTrigger>
      <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
        <SelectItem value={MODEL_DEFAULT_VALUE} textValue={defaultLabel}>
          <div className="flex flex-col">
            <span className="text-[var(--text-primary)]">{defaultLabel}</span>
            <span className="text-xs text-[var(--text-muted)]">{defaultDescription}</span>
          </div>
        </SelectItem>
        {presets.map((preset) => (
          <SelectItem key={preset.value} value={preset.value} textValue={preset.display}>
            <div className="flex flex-col">
              <span className="text-[var(--text-primary)]">{preset.display}</span>
              {preset.description && (
                <span className="text-xs text-[var(--text-muted)]">{preset.description}</span>
              )}
            </div>
          </SelectItem>
        ))}
        {hasCustomValue && value && (
          <SelectItem value={`${MODEL_CUSTOM_VALUE_PREFIX}${value}`} textValue={value}>
            <div className="flex flex-col">
              <span className="text-[var(--text-primary)]">Custom model</span>
              <span className="text-xs text-[var(--text-muted)]">{value}</span>
            </div>
          </SelectItem>
        )}
      </SelectContent>
    </Select>
  );
}

// ============================================================================
// Static config
// ============================================================================

const HARNESS_OPTIONS: {
  value: KnownHarness;
  label: string;
  description: string;
}[] = [
  {
    value: "claude",
    label: "Claude",
    description: "Current default runtime with full task pipeline support",
  },
  {
    value: "codex",
    label: "Codex",
    description: "Provider-neutral Codex harness with solo ideation and lane-level execution routing",
  },
];

const APPROVAL_POLICY_OPTIONS = [
  { value: "inherit", label: "Inherit" },
  { value: "untrusted", label: "Untrusted" },
  { value: "on-request", label: "On Request" },
  { value: "never", label: "Never" },
] as const;

const SANDBOX_MODE_OPTIONS = [
  { value: "inherit", label: "Inherit" },
  { value: "read-only", label: "Read Only" },
  { value: "workspace-write", label: "Workspace Write" },
  { value: "danger-full-access", label: "Danger Full Access" },
] as const;

const CODEX_LOCKED_APPROVAL_POLICY = "never";
const CODEX_LOCKED_SANDBOX_MODE = "danger-full-access";
const CODEX_MCP_REQUIREMENT_COPY =
  "Temporarily locked for Codex: RalphX MCP tools currently require Never approval and Danger Full Access.";

const LANE_META: Record<
  AgentLane,
  { label: string; description: string }
> = {
  ideation_primary: {
    label: "Primary Ideation",
    description: "Orchestrator and ideation lead lane",
  },
  ideation_verifier: {
    label: "Verification",
    description: "Plan verifier lane",
  },
  ideation_subagent: {
    label: "Ideation Subagents",
    description: "Specialists spawned during ideation",
  },
  ideation_verifier_subagent: {
    label: "Verifier Subagents",
    description: "Critics and specialists spawned during verification",
  },
  execution_worker: {
    label: "Execution Worker",
    description: "Primary task execution lane",
  },
  execution_reviewer: {
    label: "Execution Reviewer",
    description: "AI review lane after execution completes",
  },
  execution_reexecutor: {
    label: "Execution Re-executor",
    description: "Follow-up execution lane after review requests changes",
  },
  execution_merger: {
    label: "Execution Merger",
    description: "Merge-conflict and merge completion lane",
  },
};

const LANE_GROUPS: {
  id: "ideation" | "execution";
  title: string;
  description: string;
  lanes: AgentLane[];
}[] = [
  {
    id: "ideation",
    title: "Ideation",
    description: "Orchestration, verification, and subagent lanes used during planning.",
    lanes: IDEATION_LANES,
  },
  {
    id: "execution",
    title: "Execution Pipeline",
    description: "Task execution, review, re-execution, and merge lanes.",
    lanes: EXECUTION_LANES,
  },
];

type HarnessSectionScope = "ideation" | "execution";

function defaultsForHarness(
  lane: AgentLane,
  harness: KnownHarness,
  modelRegistry: AgentModelRegistry,
): {
  harness: Harness;
  model: string | null;
  effort: string | null;
  approvalPolicy: string | null;
  sandboxMode: string | null;
} {
  if (harness === "claude") {
    return {
      harness,
      model: null,
      effort: null,
      approvalPolicy: null,
      sandboxMode: null,
    };
  }

  const defaultCodexModel = defaultModelForProvider("codex", modelRegistry);
  const defaultCodexEffort = defaultEffortForModel(
    "codex",
    defaultCodexModel,
    modelRegistry,
  );
  if (lane === "ideation_primary") {
    return {
      harness,
      model: defaultCodexModel,
      effort: defaultCodexEffort,
      approvalPolicy: CODEX_LOCKED_APPROVAL_POLICY,
      sandboxMode: CODEX_LOCKED_SANDBOX_MODE,
    };
  }

  if (lane === "ideation_verifier") {
    return {
      harness,
      model: "gpt-5.4-mini",
      effort: "medium",
      approvalPolicy: CODEX_LOCKED_APPROVAL_POLICY,
      sandboxMode: CODEX_LOCKED_SANDBOX_MODE,
    };
  }

  if (EXECUTION_LANES.includes(lane)) {
    return {
      harness,
      model: defaultCodexModel,
      effort: defaultCodexEffort,
      approvalPolicy: CODEX_LOCKED_APPROVAL_POLICY,
      sandboxMode: CODEX_LOCKED_SANDBOX_MODE,
    };
  }

  return {
    harness,
    model: "gpt-5.4-mini",
    effort: "medium",
    approvalPolicy: CODEX_LOCKED_APPROVAL_POLICY,
    sandboxMode: CODEX_LOCKED_SANDBOX_MODE,
  };
}

function availabilityCopy(lane: AgentHarnessLaneView): string {
  if (lane.error) {
    return lane.error;
  }

  if (
    lane.configuredHarness === "codex" &&
    lane.missingCoreExecFeatures.length > 0
  ) {
    return `Missing Codex features: ${lane.missingCoreExecFeatures.join(", ")}.`;
  }

  if (lane.binaryFound && lane.binaryPath) {
    return `${lane.effectiveHarness} detected at ${lane.binaryPath}.`;
  }

  return `${lane.effectiveHarness} is the current effective harness for this lane.`;
}

function selectValue(value: string | null | undefined): string {
  return value ?? "inherit";
}

function fromSelectValue(value: string): string | null {
  return value === "inherit" ? null : value;
}

function baseLaneUpdate(lane: AgentHarnessLaneView) {
  return {
    lane: lane.lane,
    harness: lane.configuredHarness ?? lane.effectiveHarness,
    model: lane.row?.model ?? null,
    effort: lane.row?.effort ?? null,
    approvalPolicy: lane.row?.approvalPolicy ?? null,
    sandboxMode: lane.row?.sandboxMode ?? null,
  };
}

// ============================================================================
// HarnessRow
// ============================================================================

interface HarnessRowProps {
  lane: AgentHarnessLaneView;
  globalLane: AgentHarnessLaneView | null;
  disabled: boolean;
  isGlobal: boolean;
  isExpanded: boolean;
  onToggleExpanded: () => void;
  onHarnessChange: (value: KnownHarness) => void;
  onLaneChange: (
    patch: Partial<{
      model: string | null;
      effort: string | null;
      approvalPolicy: string | null;
      sandboxMode: string | null;
    }>,
  ) => void;
  modelRegistry: AgentModelRegistry;
}

function SummaryPill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-2 py-0.5 text-[0.6875rem] text-[var(--text-secondary)]">
      {children}
    </span>
  );
}

function HarnessRow({
  lane,
  globalLane,
  disabled,
  isGlobal,
  isExpanded,
  onToggleExpanded,
  onHarnessChange,
  onLaneChange,
  modelRegistry,
}: HarnessRowProps) {
  const meta = LANE_META[lane.lane];
  const configuredHarness = lane.configuredHarness ?? lane.effectiveHarness;
  const showWarning = !!lane.error || lane.missingCoreExecFeatures.length > 0;
  const showCodexControls = configuredHarness === "codex";
  const codexPolicyLocked = showCodexControls;
  const effortOptions = getEffortOptions(
    isGlobal,
    configuredHarness,
    lane.row?.model ?? null,
    modelRegistry,
  );

  // Effective model: this lane's configured model, falling back to global lane's configured model
  const effectiveModel = lane.row?.model ?? globalLane?.row?.model ?? null;
  // Effective effort: this lane's configured effort, falling back to global lane's configured effort
  const effectiveEffort = lane.row?.effort ?? globalLane?.row?.effort ?? null;

  const showEffectiveModel = !isGlobal && effectiveModel !== (lane.row?.model ?? null);
  const showEffectiveEffort = !isGlobal && effectiveEffort !== (lane.row?.effort ?? null);
  const availabilityStatusLabel = showWarning ? "Needs attention" : "Available";

  const showEffectiveHarness = lane.effectiveHarness !== configuredHarness;

  // Summary values for collapsed state
  const overrideCount = [
    lane.row?.model,
    lane.row?.effort,
    lane.row?.approvalPolicy,
    lane.row?.sandboxMode,
  ].filter((v) => v != null).length;
  const modelSummary = lane.row?.model ?? (isGlobal ? "Harness default" : null);
  const effortSummary = lane.row?.effort
    ? effortLabel(lane.row.effort)
    : isGlobal
      ? "Default"
      : null;

  return (
    <div className="py-6">
      <div className="flex items-start justify-between gap-4">
        <button
          type="button"
          onClick={onToggleExpanded}
          aria-expanded={isExpanded}
          aria-controls={`harness-body-${lane.lane}`}
          className="flex flex-1 min-w-0 items-start gap-2 text-left group"
        >
          <span className="mt-0.5 shrink-0 text-[var(--text-secondary)] transition-transform">
            {isExpanded ? (
              <ChevronDown className="w-4 h-4" />
            ) : (
              <ChevronRight className="w-4 h-4" />
            )}
          </span>
          <div className="flex-1 min-w-0">
            <span className="text-[0.9375rem] font-semibold text-[var(--text-primary)] leading-tight group-hover:text-[var(--accent-primary)] transition-colors">
              {meta.label}
            </span>
            <p className="text-xs text-[var(--text-muted)] mt-1">
              {meta.description}
            </p>
          </div>
        </button>
        <div className="shrink-0">
          <Select
            value={configuredHarness}
            onValueChange={(value) => onHarnessChange(value as KnownHarness)}
            disabled={disabled}
          >
            <SelectTrigger
              id={`harness-${lane.lane}`}
              data-testid={`harness-${lane.lane}`}
              aria-label={`${meta.label} provider`}
              className={`w-[160px] ${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)] focus:ring-[var(--accent-primary)]`}
            >
              <SelectValue placeholder="Select provider">
                <span className="truncate">
                  {HARNESS_OPTIONS.find((o) => o.value === configuredHarness)?.label ??
                    configuredHarness}
                </span>
              </SelectValue>
            </SelectTrigger>
            <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
              {HARNESS_OPTIONS.map((option) => (
                <SelectItem
                  key={option.value}
                  value={option.value}
                  textValue={option.label}
                  className="focus:bg-[var(--accent-muted)]"
                >
                  <div className="flex flex-col">
                    <span className="text-[var(--text-primary)]">{option.label}</span>
                    <span className="text-xs text-[var(--text-muted)]">
                      {option.description}
                    </span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {!isExpanded && (
        <div className="mt-3 ml-6 flex flex-wrap items-center gap-2">
          {isGlobal || overrideCount > 0 ? (
            <>
              {modelSummary && <SummaryPill>{modelSummary}</SummaryPill>}
              {effortSummary && <SummaryPill>{effortSummary}</SummaryPill>}
              {showCodexControls && lane.row?.approvalPolicy && (
                <SummaryPill>{lane.row.approvalPolicy}</SummaryPill>
              )}
              {showCodexControls && lane.row?.sandboxMode && (
                <SummaryPill>{lane.row.sandboxMode}</SummaryPill>
              )}
            </>
          ) : (
            <span className="text-[0.6875rem] text-[var(--text-muted)] italic">
              Inherited from global
            </span>
          )}
          <span
            className={`ml-auto inline-flex items-center gap-1 text-[0.6875rem] ${
              showWarning
                ? "text-[var(--warning)]"
                : "text-[var(--status-success)]"
            }`}
          >
            {showWarning ? (
              <TriangleAlert className="w-3 h-3" />
            ) : (
              <CheckCircle2 className="w-3 h-3" />
            )}
            <span>{availabilityStatusLabel}</span>
          </span>
        </div>
      )}
      <div
        id={`harness-body-${lane.lane}`}
        hidden={!isExpanded}
        className="space-y-3 mt-4 ml-6"
      >
        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1">
            <p className="text-xs font-medium text-[var(--text-secondary)]">
              Model
            </p>
            <ModelSelect
              value={lane.row?.model ?? null}
              harness={configuredHarness}
              disabled={disabled}
              onChange={(nextValue) => onLaneChange({ model: nextValue, effort: null })}
              laneLabel={meta.label}
              isGlobal={isGlobal}
              testId={`model-${lane.lane}`}
              modelRegistry={modelRegistry}
            />
            {showEffectiveModel && (
              <p className="text-[0.6875rem] text-[var(--text-muted)]">
                Effective:{" "}
                <span className="text-[var(--text-secondary)]">
                  {effectiveModel ?? "(harness default)"}
                </span>
              </p>
            )}
          </div>
          <div className="space-y-1">
            <p className="text-xs font-medium text-[var(--text-secondary)]">
              Effort
            </p>
            <Select
              value={selectValue(lane.row?.effort)}
              onValueChange={(value) => onLaneChange({ effort: fromSelectValue(value) })}
              disabled={disabled}
            >
              <SelectTrigger
                aria-label={`${meta.label} effort`}
                className={`${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)]`}
              >
                <SelectValue placeholder="Select effort">
                  <span className="truncate">{effortLabel(lane.row?.effort)}</span>
                </SelectValue>
              </SelectTrigger>
              <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
                {effortOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value} textValue={option.label}>
                    <div className="flex flex-col">
                      <span className="text-[var(--text-primary)]">{option.label}</span>
                      <span className="text-xs text-[var(--text-muted)]">{option.description}</span>
                    </div>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {showEffectiveEffort && (
              <p className="text-[0.6875rem] text-[var(--text-muted)]">
                Effective:{" "}
                <span className="text-[var(--text-secondary)]">
                  {effortLabel(effectiveEffort)}
                </span>
              </p>
            )}
          </div>
          {showCodexControls && (
            <>
              <div className="space-y-1">
                <p className="text-xs font-medium text-[var(--text-secondary)]">
                  Approval
                </p>
                <Select
                  value={selectValue(lane.row?.approvalPolicy)}
                  onValueChange={(value) =>
                    onLaneChange({ approvalPolicy: fromSelectValue(value) })
                  }
                  disabled={disabled || codexPolicyLocked}
                >
                  <SelectTrigger
                    data-testid={`approval-${lane.lane}`}
                    aria-label={`${meta.label} approval policy`}
                    className={`${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)]`}
                  >
                    <SelectValue placeholder="Select approval policy" />
                  </SelectTrigger>
                  <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
                    {APPROVAL_POLICY_OPTIONS.map((option) => (
                      <SelectItem key={option.value} value={option.value}>
                        {option.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <p className="text-xs font-medium text-[var(--text-secondary)]">
                  Sandbox
                </p>
                <Select
                  value={selectValue(lane.row?.sandboxMode)}
                  onValueChange={(value) =>
                    onLaneChange({ sandboxMode: fromSelectValue(value) })
                  }
                  disabled={disabled || codexPolicyLocked}
                >
                  <SelectTrigger
                    data-testid={`sandbox-${lane.lane}`}
                    aria-label={`${meta.label} sandbox mode`}
                    className={`${SELECT_TRIGGER_CLASS} bg-[var(--bg-surface)] border-[var(--border-default)]`}
                  >
                    <SelectValue placeholder="Select sandbox mode" />
                  </SelectTrigger>
                  <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
                    {SANDBOX_MODE_OPTIONS.map((option) => (
                      <SelectItem key={option.value} value={option.value}>
                        {option.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </>
          )}
        </div>
        {showCodexControls && (
          <InlineNotice tone="info">{CODEX_MCP_REQUIREMENT_COPY}</InlineNotice>
        )}
        <InlineNotice
          tone={showWarning ? "warn" : "ok"}
          title={
            showEffectiveHarness
              ? `${availabilityStatusLabel} · Effective: ${lane.effectiveHarness}`
              : availabilityStatusLabel
          }
        >
          {availabilityCopy(lane)}
        </InlineNotice>
      </div>
    </div>
  );
}

// ============================================================================
// HarnessSubsection
// ============================================================================

function HarnessSubsection({
  projectId,
  projectName,
  scope,
  globalLanes,
  isGlobal,
  tabValue,
  modelRegistry,
}: {
  projectId: string | null;
  projectName: string | null;
  scope: HarnessSectionScope;
  globalLanes: AgentHarnessLaneView[];
  isGlobal: boolean;
  tabValue: HarnessTabValue;
  modelRegistry: AgentModelRegistry;
}) {
  const [showError, setShowError] = useState(false);
  const {
    lanes,
    isPlaceholderData,
    updateLane,
    saveError,
  } = useAgentHarnessSettings(projectId);
  const isDisabled = !isGlobal && projectId === null;

  // Progressive disclosure: persisted per-tab. Defaults = global open, project
  // collapsed. Warnings force a lane open regardless of the stored preference.
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  useEffect(() => {
    const laneIds = lanes.map((l) => l.lane);
    const persisted = loadHarnessExpanded(tabValue, laneIds);
    setExpanded((prev) => {
      const next: Record<string, boolean> = { ...prev };
      let changed = false;
      lanes.forEach((lane) => {
        const hasWarning =
          !!lane.error || lane.missingCoreExecFeatures.length > 0;
        const current = next[lane.lane];
        if (current === undefined) {
          const stored = persisted[lane.lane];
          next[lane.lane] =
            stored !== undefined ? stored || hasWarning : isGlobal || hasWarning;
          changed = true;
        } else if (hasWarning && !current) {
          next[lane.lane] = true;
          changed = true;
        }
      });
      return changed ? next : prev;
    });
  }, [lanes, isGlobal, tabValue]);

  const toggleLane = (laneId: string) => {
    setExpanded((prev) => {
      const nextValue = !prev[laneId];
      saveHarnessExpanded(tabValue, laneId, nextValue);
      return { ...prev, [laneId]: nextValue };
    });
  };

  const setAllExpanded = (value: boolean) => {
    const laneIds = lanes.map((l) => l.lane);
    saveHarnessExpandedBulk(tabValue, laneIds, value);
    setExpanded(Object.fromEntries(laneIds.map((id) => [id, value])));
  };

  const allExpanded = lanes.length > 0 && lanes.every((l) => expanded[l.lane]);

  const handleHarnessChange = (lane: AgentLane, harness: KnownHarness) => {
    if (isDisabled) {
      return;
    }

    setShowError(false);
    updateLane(
      {
        lane,
        ...defaultsForHarness(lane, harness, modelRegistry),
      },
      { onError: () => setShowError(true) },
    );
  };

  const handleLaneSettingsChange = (
    laneView: AgentHarnessLaneView,
    patch: Partial<{
      model: string | null;
      effort: string | null;
      approvalPolicy: string | null;
      sandboxMode: string | null;
    }>,
  ) => {
    if (isDisabled) {
      return;
    }

    setShowError(false);
    updateLane(
      {
        ...baseLaneUpdate(laneView),
        ...patch,
      },
      { onError: () => setShowError(true) },
    );
  };

  return (
    <div>
      <div className="flex items-start justify-between gap-3 mb-3">
        <p className="text-xs text-[var(--text-muted)] flex-1">
          {isGlobal
            ? scope === "execution"
              ? "Default harness policy for execution worker, reviewer, re-executor, and merger lanes."
              : "Default harness policy for ideation leads, verification, and specialist lanes."
            : projectId !== null
              ? `Overrides for ${projectName ?? "the active project"}. Leave blank to inherit global defaults.`
              : scope === "execution"
                ? "Select a project to override execution-pipeline agents for a specific project."
                : "Select a project to override ideation agents for a specific project."}
        </p>
        {lanes.length > 0 && (
          <button
            type="button"
            onClick={() => setAllExpanded(!allExpanded)}
            className="shrink-0 text-[0.6875rem] font-medium text-[var(--text-secondary)] hover:text-[var(--accent-primary)] transition-colors"
          >
            {allExpanded ? "Collapse all" : "Expand all"}
          </button>
        )}
      </div>

      {showError && saveError && (
        <ErrorBanner
          error={saveError.message ?? "Failed to save agent harness settings"}
          onDismiss={() => setShowError(false)}
        />
      )}

      <div
        className={isDisabled ? "opacity-50 pointer-events-none" : undefined}
      >
        {LANE_GROUPS.filter((group) => group.id === scope).map((group) => {
          const groupLanes = lanes.filter((lane) => group.lanes.includes(lane.lane));
          if (groupLanes.length === 0) {
            return null;
          }

          const scopeKey = isGlobal ? "global" : "project";
          return (
            <div
              key={`${scopeKey}-${group.id}`}
              className="divide-y divide-[var(--border-default)]"
            >
              {groupLanes.map((lane) => {
                const globalLane = isGlobal
                  ? null
                  : (globalLanes.find((g) => g.lane === lane.lane) ?? null);
                return (
                  <HarnessRow
                    key={`${scopeKey}-${lane.lane}`}
                    lane={lane}
                    globalLane={globalLane}
                    disabled={isDisabled || isPlaceholderData}
                    isGlobal={isGlobal}
                    isExpanded={expanded[lane.lane] ?? false}
                    onToggleExpanded={() => toggleLane(lane.lane)}
                    onHarnessChange={(value) => handleHarnessChange(lane.lane, value)}
                    onLaneChange={(patch) => handleLaneSettingsChange(lane, patch)}
                    modelRegistry={modelRegistry}
                  />
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ============================================================================
// AgentHarnessSection
// ============================================================================

function AgentHarnessSection({
  scope,
  title,
  description,
}: {
  scope: HarnessSectionScope;
  title: string;
  description: string;
}) {
  const activeProject = useProjectStore(selectActiveProject);
  const projectId = activeProject?.id ?? null;
  const projectName = activeProject?.name ?? null;
  const { registry: modelRegistry } = useAgentModels();

  // Fetch global lanes for effective value resolution in project rows
  const { lanes: globalLanes } = useAgentHarnessSettings(null);

  const [activeTab, setActiveTabState] = useState<HarnessTabValue>(() =>
    loadHarnessTab(scope),
  );
  const setActiveTab = (tab: HarnessTabValue) => {
    setActiveTabState(tab);
    saveHarnessTab(scope, tab);
  };

  return (
    <SectionCard
      icon={<Cpu className="w-5 h-5 text-[var(--accent-primary)]" />}
      title={title}
      description={description}
    >
      <Tabs
        value={activeTab}
        onValueChange={(v) => setActiveTab(v as HarnessTabValue)}
        className="w-full"
      >
        <TabsList className="inline-flex h-9 items-center rounded-md bg-[var(--bg-surface)] p-1 text-[var(--text-secondary)] border border-[var(--border-subtle)]">
          <TabsTrigger
            value="global"
            className="rounded-sm px-3 py-1 text-xs font-medium data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm"
          >
            Global Defaults
          </TabsTrigger>
          <TabsTrigger
            value="project"
            className="rounded-sm px-3 py-1 text-xs font-medium data-[state=active]:bg-[var(--bg-elevated)] data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-sm"
          >
            Project Overrides
          </TabsTrigger>
        </TabsList>
        <TabsContent value="global" className="mt-4">
          <HarnessSubsection
            projectId={null}
            projectName={null}
            scope={scope}
            globalLanes={globalLanes}
            isGlobal={true}
            tabValue="global"
            modelRegistry={modelRegistry}
          />
        </TabsContent>
        <TabsContent value="project" className="mt-4">
          <HarnessSubsection
            projectId={projectId}
            projectName={projectName}
            scope={scope}
            globalLanes={globalLanes}
            isGlobal={false}
            tabValue="project"
            modelRegistry={modelRegistry}
          />
        </TabsContent>
      </Tabs>
    </SectionCard>
  );
}

export function IdeationHarnessSection() {
  return (
    <AgentHarnessSection
      scope="ideation"
      title="Ideation Agents"
      description="Choose Claude or Codex for ideation leads, verification, and specialist lanes. Codex ideation still runs in solo mode, so these settings mainly control planning and verifier routing."
    />
  );
}

export function ExecutionHarnessSection() {
  return (
    <AgentHarnessSection
      scope="execution"
      title="Execution Pipeline Agents"
      description="Choose Claude or Codex for the worker, reviewer, re-executor, and merger lanes. These settings control the live execution pipeline, including Codex approval and sandbox behavior per lane."
    />
  );
}
