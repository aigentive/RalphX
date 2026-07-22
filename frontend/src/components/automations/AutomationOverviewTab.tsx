import { type ReactNode, useMemo } from "react";
import { ChevronDown, Copy, ExternalLink, GitPullRequest } from "lucide-react";
import { toast } from "sonner";

import type {
  Automation,
  AutomationPipelineProgress as AutomationPipelineProgressData,
  AutomationRun,
  AutomationUsage,
} from "@/api/automations";
import { isOpenAutomationRun } from "@/components/automations/automationStage";
import {
  AUTOMATION_PHASES_LABEL,
  parseAutomationGoalItems,
} from "@/components/automations/automationGoalItems";
import { AutomationPhaseProgress } from "@/components/automations/AutomationPhases";
import { AutomationRunPrLink } from "@/components/automations/AutomationRunPrLink";
import { AutomationSpecView } from "@/components/automations/AutomationSpecView";
import { AutomationDetailsTabs } from "@/components/automations/AutomationDetailsTabs";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatDate, numberField, parseRecord, stringField } from "./automationDetailFormat";
import { ExpandableText, Pill, Section } from "./automationDetailShared";

function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}

function formatEstimatedUsd(value: AutomationUsage["estimatedUsd"]): string {
  if (value === null) {
    return "Not recorded";
  }
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 4,
  }).format(value);
}

function formatBase(automation: Automation): string {
  return (automation.baseDisplayName ?? automation.baseRef) || automation.baseRefKind;
}

function formatMode(automation: Automation): string {
  const effort = automation.logicalEffort ? `/${automation.logicalEffort}` : "";
  return `${automation.runMode} · ${automation.providerHarness}/${automation.modelId}${effort}`;
}

/** Newest run that carries PR metadata, for the config "Current PR" row. */
function newestPrRun(runs: AutomationRun[]): AutomationRun | null {
  return runs.reduce<AutomationRun | null>(
    (newest, run) =>
      (run.prNumber !== null || run.prUrl !== null) &&
      (!newest || run.runIndex > newest.runIndex)
        ? run
        : newest,
    null,
  );
}

/**
 * Goal-item id → newest plan artifact id, derived strictly from stamped run
 * linkage (`goalItemId`). Unmapped (pre-migration) runs contribute nothing, so
 * historical items never get fabricated icons.
 */
function planArtifactsByGoalItem(runs: AutomationRun[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const run of [...runs].sort((a, b) => b.runIndex - a.runIndex)) {
    if (run.goalItemId && run.planArtifactId && !map.has(run.goalItemId)) {
      map.set(run.goalItemId, run.planArtifactId);
    }
  }
  return map;
}

function ConfigGroup({
  title,
  items,
  testId,
}: {
  title: string;
  items: Array<[string, ReactNode]>;
  testId: string;
}) {
  return (
    <div data-testid={testId}>
      {title ? (
        <div
          className="mb-2 text-xs font-semibold uppercase tracking-normal"
          style={{ color: "var(--text-muted)" }}
        >
          {title}
        </div>
      ) : null}
      <KeyValueList items={items} />
    </div>
  );
}

function KeyValueList({ items }: { items: Array<[string, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {items.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <dt className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
            {label}
          </dt>
          <dd className="mt-1 min-w-0 text-sm" style={{ color: "var(--text-secondary)" }}>
            {typeof value === "string" || typeof value === "number" ? (
              <span className="block truncate">{value}</span>
            ) : (
              value
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function PipelineProgress({ pipeline }: { pipeline: AutomationPipelineProgressData }) {
  return (
    <div
      className="mt-4 rounded-md p-3"
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automation-pipeline-progress"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <div className="text-xs font-semibold uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
            Task pipeline
          </div>
          <div className="mt-1 text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            {pipeline.taskMerged} / {pipeline.taskTotal} merged
          </div>
        </div>
        <Pill label={pipeline.status} status={pipeline.status} />
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full" style={{ backgroundColor: "var(--border-default)" }}>
        <div
          className="h-full rounded-full"
          style={{
            backgroundColor: "var(--accent-primary)",
            width: `${pipeline.taskTotal === 0 ? 0 : Math.round((pipeline.taskMerged / pipeline.taskTotal) * 100)}%`,
          }}
        />
      </div>
      <div className="mt-3 space-y-2">
        {pipeline.tasks.map((task) => (
          <div key={task.id} className="flex min-w-0 items-center justify-between gap-3 text-xs">
            <span className="min-w-0 truncate" style={{ color: "var(--text-secondary)" }}>
              {task.title}
            </span>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
              {task.blockedBy.length === 0
                ? task.status
                : `${task.blockedBy.length} ${task.blockedBy.length === 1 ? "dependency" : "dependencies"}`}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function BranchConfigValue({ automation }: { automation: Automation }) {
  const branchRef = automation.baseRef.trim();
  if (!branchRef) {
    return <span className="block truncate">Not recorded</span>;
  }
  const displayName = automation.baseDisplayName?.trim();
  const showDisplayName = Boolean(displayName && displayName !== branchRef);
  const copyBranch = async () => {
    try {
      if (!navigator.clipboard) {
        throw new Error("clipboard unavailable");
      }
      await navigator.clipboard.writeText(branchRef);
      toast.success("Branch copied");
    } catch {
      toast.error("Failed to copy branch");
    }
  };

  return (
    <span className="inline-flex max-w-full items-center gap-1.5 align-middle">
      <span className="min-w-0 truncate">
        {showDisplayName ? (
          <span className="mr-1" style={{ color: "var(--text-muted)" }}>
            {displayName}
          </span>
        ) : null}
        <code className="font-mono text-[0.8125rem]" data-testid="automation-branch-value">
          {branchRef}
        </code>
      </span>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Copy branch"
            className="h-6 w-6 shrink-0"
            onClick={() => void copyBranch()}
            data-testid="automation-branch-copy"
          >
            <Copy className="h-3.5 w-3.5" aria-hidden="true" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Copy branch</TooltipContent>
      </Tooltip>
    </span>
  );
}

function SourcePrInput({ automation }: { automation: Automation }) {
  const sourcePr = parseRecord(automation.baseSourcePullRequestJson);
  const number = numberField(sourcePr, "number");
  const title = stringField(sourcePr, "title");
  const url = stringField(sourcePr, "url");

  if (!sourcePr) {
    return (
      <p className="text-sm" style={{ color: "var(--text-muted)" }}>
        No setup input references are attached to this automation record.
      </p>
    );
  }

  return (
    <div className="flex flex-wrap items-center gap-2 text-sm" style={{ color: "var(--text-secondary)" }}>
      <GitPullRequest className="h-4 w-4" aria-hidden="true" />
      <span>{number ? `PR #${number}` : "Source PR"}</span>
      {title && <span className="truncate">{title}</span>}
      {url && (
        <a
          href={url}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1 text-[var(--accent-primary)]"
        >
          Open <ExternalLink className="h-3 w-3" aria-hidden="true" />
        </a>
      )}
    </div>
  );
}

function GoalItems({
  value,
  planByGoalItemId,
}: {
  value: string | null;
  planByGoalItemId: ReadonlyMap<string, string>;
}) {
  const parsed = useMemo(
    () => parseAutomationGoalItems(value, { limit: 6 }),
    [value],
  );

  if (parsed.length === 0) {
    return null;
  }

  return (
    <div className="mt-4 space-y-2">
      <div className="text-xs font-medium uppercase tracking-normal" style={{ color: "var(--text-muted)" }}>
        {AUTOMATION_PHASES_LABEL}
      </div>
      <AutomationPhaseProgress
        value={value}
        planByGoalItemId={planByGoalItemId}
      />
    </div>
  );
}

export interface AutomationOverviewTabProps {
  automation: Automation;
  runs: AutomationRun[];
  usage: AutomationUsage;
  pipeline: AutomationPipelineProgressData | null;
  canOpenSetupConversation: boolean;
  onOpenSetupConversation: () => void;
}

export function AutomationOverviewTab({
  automation,
  runs,
  usage,
  pipeline,
  canOpenSetupConversation,
  onOpenSetupConversation,
}: AutomationOverviewTabProps) {
  const planByGoalItemId = useMemo(() => planArtifactsByGoalItem(runs), [runs]);
  const prRun = newestPrRun(runs);

  return (
    <div className="grid gap-4 lg:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
      <div className="space-y-4">
        <Section title="Goal" testId="automation-goal-card">
          <ExpandableText text={automation.goalPrompt} />
          <GoalItems
            value={automation.goalItemsJson}
            planByGoalItemId={planByGoalItemId}
          />
          {pipeline ? <PipelineProgress pipeline={pipeline} /> : null}
        </Section>
      </div>
      <div className="space-y-4">
        <AutomationDetailsTabs
          hasSpec={Boolean(automation.specArtifactId)}
          inputCount={parseRecord(automation.baseSourcePullRequestJson) ? 1 : 0}
          spec={<AutomationSpecView specArtifactId={automation.specArtifactId} />}
          inputs={<SourcePrInput automation={automation} />}
          config={(
            <>
              <div className="space-y-4">
                <ConfigGroup
                  title="Execution"
                  testId="automation-config-group-execution"
                  items={[
                    ["Mode / model", formatMode(automation)] as [string, ReactNode],
                    [
                      "Setup conversation",
                      automation.setupConversationId ? (
                        <Button
                          type="button"
                          variant="link"
                          className="h-auto gap-1 p-0 text-sm"
                          disabled={!canOpenSetupConversation}
                          onClick={onOpenSetupConversation}
                          data-testid="automation-setup-conversation-link"
                        >
                          Open setup conversation
                          <ExternalLink className="h-3 w-3" aria-hidden="true" />
                        </Button>
                      ) : (
                        "Not recorded"
                      ),
                    ] as [string, ReactNode],
                    // One-click path to the automation's PR on GitHub —
                    // previously only reachable via the run conversation.
                    ...(prRun
                      ? [[
                          isOpenAutomationRun(prRun) ? "Current PR" : "Last PR",
                          <AutomationRunPrLink
                            run={prRun}
                            testId="automation-config-pr-link"
                          />,
                        ] as [string, ReactNode]]
                      : []),
                    ["Base", formatBase(automation)] as [string, ReactNode],
                    ...(automation.baseRef.trim()
                      ? [["Branch", <BranchConfigValue automation={automation} />] as [string, ReactNode]]
                      : []),
                    ["Chain mode", automation.chainMode] as [string, ReactNode],
                    ["Completion signal", automation.completionSignal] as [string, ReactNode],
                  ]}
                />
                <ConfigGroup
                  title="Limits"
                  testId="automation-config-group-limits"
                  items={[
                    ["Max runs", `${runs.length} / ${automation.maxRuns}`],
                    ["Max failures", automation.maxConsecutiveFailures],
                  ]}
                />
                <Collapsible defaultOpen={usage.estimatedUsd !== null}>
                  <CollapsibleTrigger asChild>
                    <button
                      type="button"
                      className="group flex w-full items-center gap-1 text-left"
                      data-testid="automation-config-usage-toggle"
                    >
                      <span
                        className="text-xs font-semibold uppercase tracking-normal"
                        style={{ color: "var(--text-muted)" }}
                      >
                        Usage
                      </span>
                      <ChevronDown
                        className="h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180"
                        style={{ color: "var(--text-muted)" }}
                        aria-hidden="true"
                      />
                    </button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="mt-2">
                    <ConfigGroup
                      title=""
                      testId="automation-config-group-usage"
                      items={[
                        ["Input tokens", formatNumber(usage.inputTokens)] as [string, ReactNode],
                        ["Output tokens", formatNumber(usage.outputTokens)] as [string, ReactNode],
                        ["Cache tokens", formatNumber(usage.cacheCreationTokens + usage.cacheReadTokens)] as [string, ReactNode],
                        ...(usage.estimatedUsd !== null
                          ? [["Estimated cost", formatEstimatedUsd(usage.estimatedUsd)] as [string, ReactNode]]
                          : []),
                      ]}
                    />
                  </CollapsibleContent>
                </Collapsible>
                <p
                  className="text-xs"
                  style={{ color: "var(--text-muted)" }}
                  data-testid="automation-config-timestamps"
                >
                  Created {formatDate(automation.createdAt)} · Updated {formatDate(automation.updatedAt)}
                </p>
              </div>
              {automation.pausedReasonCode && (
                <div className="mt-4 rounded-md p-3 text-sm" style={{
                  backgroundColor: "var(--bg-hover)",
                  color: "var(--text-secondary)",
                }}>
                  Paused: {automation.pausedReasonCode}
                  {automation.pausedReasonDetail ? ` - ${automation.pausedReasonDetail}` : ""}
                </div>
              )}
            </>
          )}
        />
      </div>
    </div>
  );
}
