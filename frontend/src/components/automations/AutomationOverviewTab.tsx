import { type ReactNode, useMemo, useState } from "react";
import {
  ChevronDown,
  ExternalLink,
  FileText,
  GitPullRequest,
  Pause,
} from "lucide-react";

import type {
  Automation,
  AutomationPipelineProgress as AutomationPipelineProgressData,
  AutomationRun,
  AutomationUsage,
} from "@/api/automations";
import { AutomationDetailPrChip } from "@/components/automations/AutomationDetailPrChip";
import { AutomationPhaseList } from "@/components/automations/AutomationPhaseList";
import { AutomationPlanDialog } from "@/components/automations/AutomationPlanDialog";
import { getTrailingFailureStreak } from "@/components/automations/automationDetailPresentation";
import { describePausedReason } from "@/components/automations/automationRunView";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { CopyableRef } from "@/components/ui/copyable-ref";
import { NoticeBanner } from "@/components/ui/notice-banner";
import { Separator } from "@/components/ui/separator";
import { formatDate, numberField, parseRecord, stringField } from "./automationDetailFormat";
import { ExpandableText, FieldLabel, Pill, Section } from "./automationDetailShared";

function formatNumber(value: number): string {
  return new Intl.NumberFormat().format(value);
}

function formatEstimatedUsd(value: AutomationUsage["estimatedUsd"]): string {
  if (value === null) return "Not recorded";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 4,
  }).format(value);
}

function formatBase(automation: Automation): string {
  const target = automation.baseTargetDisplayName ?? automation.baseTargetRef;
  if (target?.trim()) return target;
  return (automation.baseDisplayName ?? automation.baseRef) || automation.baseRefKind;
}

function newestPrRun(runs: AutomationRun[]): AutomationRun | null {
  return [...runs]
    .sort((left, right) => right.runIndex - left.runIndex)
    .find((run) => run.prNumber !== null || run.prUrl !== null) ?? null;
}

function DetailRows({
  items,
  testId,
}: {
  items: Array<[string, ReactNode]>;
  testId: string;
}) {
  return (
    <dl className="space-y-3" data-testid={testId}>
      {items.map(([label, value]) => (
        <div
          key={label}
          className="grid min-w-0 grid-cols-[minmax(7rem,0.8fr)_minmax(0,1.2fr)] items-start gap-3"
        >
          <dt><FieldLabel>{label}</FieldLabel></dt>
          <dd
            className="min-w-0 text-right text-sm tabular-nums"
            style={{ color: "var(--text-primary)" }}
          >
            {typeof value === "string" || typeof value === "number"
              ? <span className="block truncate">{value}</span>
              : value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function BranchValue({ automation }: { automation: Automation }) {
  const branchRef = automation.baseRef.trim();
  if (!branchRef) return <span>Not recorded</span>;
  const displayName = automation.baseDisplayName?.trim();
  return (
    <CopyableRef
      value={branchRef}
      ariaLabel="Copy branch"
      testId="automation-branch"
      {...(displayName && displayName !== branchRef ? { prefixLabel: displayName } : {})}
    />
  );
}

function SourcePrInput({ automation }: { automation: Automation }) {
  const sourcePr = parseRecord(automation.baseSourcePullRequestJson);
  const number = numberField(sourcePr, "number");
  const title = stringField(sourcePr, "title");
  const url = stringField(sourcePr, "url");

  if (!sourcePr) {
    return (
      <div className="flex items-start gap-2 py-1">
        <GitPullRequest
          className="mt-0.5 h-4 w-4 shrink-0"
          style={{ color: "var(--text-subtle)" }}
          aria-hidden="true"
        />
        <p className="text-sm" style={{ color: "var(--text-muted)" }}>
          No setup input references are attached.
        </p>
      </div>
    );
  }

  return (
    <div
      className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3"
      data-testid="automation-source-pr-input"
    >
      <GitPullRequest className="h-4 w-4" style={{ color: "var(--text-muted)" }} />
      <div className="min-w-0">
        <div className="text-xs" style={{ color: "var(--text-muted)" }}>
          Source pull request
        </div>
        <div className="mt-0.5 flex min-w-0 gap-2 text-sm">
          <span className="shrink-0 font-medium" style={{ color: "var(--text-primary)" }}>
            {number ? `PR #${number}` : "Source PR"}
          </span>
          {title ? (
            <span className="truncate" style={{ color: "var(--text-secondary)" }}>
              {title}
            </span>
          ) : null}
        </div>
      </div>
      {url ? (
        <a
          href={url}
          target="_blank"
          rel="noreferrer"
          aria-label={`Open ${number ? `PR #${number}` : "source PR"}`}
          className="inline-flex items-center gap-1 text-xs text-[var(--accent-primary)] hover:text-[var(--accent-secondary)]"
        >
          Open <ExternalLink className="h-3 w-3" aria-hidden="true" />
        </a>
      ) : null}
    </div>
  );
}

function PipelineProgress({ pipeline }: { pipeline: AutomationPipelineProgressData }) {
  const percent = pipeline.taskTotal === 0
    ? 0
    : Math.round((pipeline.taskMerged / pipeline.taskTotal) * 100);
  return (
    <div
      className="mt-4 rounded-md p-3"
      style={{
        backgroundColor: "var(--bg-hover, #2a2a31)",
        borderColor: "var(--border-default, #393940)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="automation-pipeline-progress"
    >
      <div className="flex items-center justify-between gap-2">
        <div>
          <FieldLabel variant="group" className="block">Task pipeline</FieldLabel>
          <div className="mt-1 text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            {pipeline.taskMerged} / {pipeline.taskTotal} merged
          </div>
        </div>
        <Pill label={pipeline.status} status={pipeline.status} />
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full" style={{ backgroundColor: "var(--border-default)" }}>
        <div
          className="h-full rounded-full"
          style={{ backgroundColor: "var(--accent-primary)", width: `${percent}%` }}
        />
      </div>
      <div className="mt-3 space-y-2">
        {pipeline.tasks.map((task) => (
          <div key={task.id} className="flex items-center justify-between gap-3 text-xs">
            <span className="truncate" style={{ color: "var(--text-secondary)" }}>
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

function FailureLimit({
  current,
  maximum,
}: {
  current: number;
  maximum: number;
}) {
  return (
    <div>
      <div className="flex items-center justify-between gap-3">
        <FieldLabel>Consecutive failures</FieldLabel>
        <span className="text-xs tabular-nums" style={{ color: "var(--text-muted)" }}>
          {current} / {maximum}
        </span>
      </div>
      <div
        className="mt-2 flex gap-1"
        role="meter"
        aria-label="Consecutive failures"
        aria-valuenow={current}
        aria-valuemin={0}
        aria-valuemax={maximum}
      >
        {Array.from({ length: maximum }, (_, index) => (
          <span
            key={index}
            className="h-2 flex-1 rounded-sm"
            style={{
              backgroundColor: index < current
                ? "var(--status-error, #dd3c3c)"
                : "var(--bg-hover, #2a2a31)",
            }}
          />
        ))}
      </div>
      <p className="mt-2 text-xs" style={{ color: "var(--text-muted)" }}>
        Auto-pauses when this limit is reached.
      </p>
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
  const [specOpen, setSpecOpen] = useState(false);
  const prRun = useMemo(() => newestPrRun(runs), [runs]);
  const failureStreak = useMemo(() => getTrailingFailureStreak(runs), [runs]);
  const inputCount = parseRecord(automation.baseSourcePullRequestJson) ? 1 : 0;
  const runPercent = automation.maxRuns === 0
    ? 0
    : Math.min(100, Math.round((runs.length / automation.maxRuns) * 100));

  const setupConversation = automation.setupConversationId ? (
    <Button
      type="button"
      variant="link"
      className="h-auto justify-end gap-1 p-0 text-sm"
      disabled={!canOpenSetupConversation}
      onClick={onOpenSetupConversation}
      data-testid="automation-setup-conversation-link"
    >
      Open setup
      <ExternalLink className="h-3 w-3" aria-hidden="true" />
    </Button>
  ) : "Not recorded";

  return (
    <>
      {automation.pausedReasonCode ? (
        <NoticeBanner
          tone="warning"
          icon={<Pause className="h-4 w-4" aria-hidden="true" />}
          title={`Paused: ${describePausedReason(automation.pausedReasonCode)}.`}
          className="mb-4"
          testId="automation-paused-reason"
        >
          {automation.pausedReasonDetail ?? "Automation scheduling is paused."}
        </NoticeBanner>
      ) : null}

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1.15fr)_minmax(20rem,0.85fr)]">
        <div className="space-y-4">
          <Section
            title="Goal"
            testId="automation-goal-card"
            background="surface"
            headerRight={(
              <span className="text-xs" style={{ color: "var(--text-muted)" }}>
                completes on {automation.completionSignal.replace(/_/g, " ")}
              </span>
            )}
          >
            <ExpandableText text={automation.goalPrompt} />
          </Section>

          <Section
            title="Phases"
            testId="automation-phases-card"
            background="surface"
          >
            <AutomationPhaseList value={automation.goalItemsJson} runs={runs} />
            {pipeline ? <PipelineProgress pipeline={pipeline} /> : null}
          </Section>
        </div>

        <div className="space-y-4">
          <Section title="Execution" testId="automation-execution-card" background="surface">
            <DetailRows
              testId="automation-config-group-execution"
              items={[
                ["Mode", automation.runMode],
                [
                  "Model / effort",
                  `${automation.providerHarness}/${automation.modelId}${automation.logicalEffort ? ` · ${automation.logicalEffort}` : ""}`,
                ],
                ["Chain mode", automation.chainMode.replace(/_/g, " ")],
                ["Completion", automation.completionSignal.replace(/_/g, " ")],
              ]}
            />
            <Separator className="my-4 bg-[var(--border-subtle)]" />
            <FieldLabel variant="group" className="mb-3 block">Source</FieldLabel>
            <DetailRows
              testId="automation-config-group-source"
              items={[
                ["Base", formatBase(automation)],
                ["Working branch", <BranchValue automation={automation} />],
                [
                  "Last PR",
                  prRun
                    ? <AutomationDetailPrChip run={prRun} testId="automation-config-pr-link" />
                    : "Not published",
                ],
                ["Setup conversation", setupConversation],
              ]}
            />
            <Separator className="my-4 bg-[var(--border-subtle)]" />
            <div data-testid="automation-config-group-limits">
              <FieldLabel variant="group" className="mb-3 block">Limits</FieldLabel>
              <div className="flex items-center justify-between gap-3 text-sm">
                <span style={{ color: "var(--text-secondary)" }}>Max runs</span>
                <span className="tabular-nums" style={{ color: "var(--text-primary)" }}>
                  {runs.length} / {automation.maxRuns}
                </span>
              </div>
              <div
                className="mt-2 h-1.5 overflow-hidden rounded-full"
                style={{ backgroundColor: "var(--bg-hover)" }}
                role="progressbar"
                aria-label="Maximum runs used"
                aria-valuenow={runs.length}
                aria-valuemin={0}
                aria-valuemax={automation.maxRuns}
              >
                <div
                  className="h-full rounded-full"
                  style={{ backgroundColor: "var(--accent-primary)", width: `${runPercent}%` }}
                />
              </div>
              <div className="mt-4">
                <FailureLimit
                  current={failureStreak}
                  maximum={automation.maxConsecutiveFailures}
                />
              </div>
            </div>
            <Separator className="my-4 bg-[var(--border-subtle)]" />
            <Collapsible defaultOpen={usage.estimatedUsd !== null}>
              <CollapsibleTrigger asChild>
                <button
                  type="button"
                  className="group flex w-full items-center gap-1 text-left"
                  data-testid="automation-config-usage-toggle"
                >
                  <FieldLabel variant="group">Usage</FieldLabel>
                  <ChevronDown
                    className="h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180"
                    style={{ color: "var(--text-muted)" }}
                    aria-hidden="true"
                  />
                </button>
              </CollapsibleTrigger>
              <CollapsibleContent className="mt-3">
                <DetailRows
                  testId="automation-config-group-usage"
                  items={[
                    ["Input tokens", formatNumber(usage.inputTokens)],
                    ["Output tokens", formatNumber(usage.outputTokens)],
                    ["Cache tokens", formatNumber(usage.cacheCreationTokens + usage.cacheReadTokens)],
                    ...(usage.estimatedUsd !== null
                      ? [["Estimated cost", formatEstimatedUsd(usage.estimatedUsd)] as [string, ReactNode]]
                      : []),
                  ]}
                />
              </CollapsibleContent>
            </Collapsible>
            <p
              className="mt-4 text-xs"
              style={{ color: "var(--text-muted)" }}
              data-testid="automation-config-timestamps"
            >
              Created {formatDate(automation.createdAt)} · Updated {formatDate(automation.updatedAt)}
            </p>
          </Section>

          <Section
            title="Spec & inputs"
            testId="automation-spec-inputs-card"
            background="surface"
            headerRight={(
              <span className="text-xs" style={{ color: "var(--text-muted)" }}>
                {inputCount} {inputCount === 1 ? "input" : "inputs"}
              </span>
            )}
          >
            {automation.specArtifactId ? (
              <Button
                type="button"
                variant="outline"
                className="mb-4 w-full justify-start gap-2"
                onClick={() => setSpecOpen(true)}
                data-testid="automation-spec-chip"
              >
                <FileText className="h-4 w-4" aria-hidden="true" />
                Automation spec
                <span className="ml-auto text-xs" style={{ color: "var(--text-muted)" }}>
                  View document
                </span>
              </Button>
            ) : (
              <p className="mb-4 text-sm" style={{ color: "var(--text-muted)" }}>
                No specification artifact is linked.
              </p>
            )}
            <SourcePrInput automation={automation} />
          </Section>
        </div>
      </div>

      <AutomationPlanDialog
        planArtifactId={automation.specArtifactId}
        heading="Automation spec"
        title="Linked automation specification"
        open={specOpen}
        onOpenChange={setSpecOpen}
      />
    </>
  );
}
