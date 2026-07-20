import type { AutomationJudgeState, AutomationRunStatus } from "@/api/automations";

export type AutomationConversationSurface = "setup" | "run";

export type AutomationConversationTabId =
  | "automation"
  | "plan"
  | "pr"
  | "issues"
  | "verification"
  | "tasks"
  | "review"
  | "publish"
  | "jira"
  | "linear"
  | "granola";

export interface AutomationConversationPolicyTab {
  id: AutomationConversationTabId;
  enabled: boolean;
  disabledReason?: string | undefined;
}

export interface AutomationConversationTabAvailability {
  hasPlanArtifact: boolean;
  hasPullRequest: boolean;
  hasPublishWorkspace?: boolean | undefined;
  hasIssues?: boolean | undefined;
  hasVerification?: boolean | undefined;
  hasTasks?: boolean | undefined;
  hasReview?: boolean | undefined;
  hasJira?: boolean | undefined;
  hasLinear?: boolean | undefined;
  hasGranola?: boolean | undefined;
  canStartPlan?: boolean | undefined;
}

export interface AutomationConversationTabPolicyInput {
  surface: AutomationConversationSurface;
  runStatus: AutomationRunStatus | null;
  judgeState: AutomationJudgeState | null;
  workspaceMode: string | null;
  availability: AutomationConversationTabAvailability;
  /** Caller-directed destinations (for example, an automation notification) win over inferred run state. */
  tabHint?: AutomationConversationTabId | undefined;
}

export interface AutomationConversationTabPolicy {
  tabs: AutomationConversationPolicyTab[];
  defaultTab: AutomationConversationTabId;
}

function enabledTab(id: AutomationConversationTabId): AutomationConversationPolicyTab {
  return { id, enabled: true };
}

function disabledTab(
  id: AutomationConversationTabId,
  disabledReason: string,
): AutomationConversationPolicyTab {
  return { id, enabled: false, disabledReason };
}

function setupTabs(
  availability: AutomationConversationTabAvailability,
): AutomationConversationPolicyTab[] {
  return [
    ...(availability.hasIssues ? [enabledTab("issues")] : []),
    ...(availability.hasPlanArtifact || availability.canStartPlan
      ? [enabledTab("plan")]
      : []),
    ...(availability.hasVerification ? [enabledTab("verification")] : []),
    ...(availability.hasTasks ? [enabledTab("tasks")] : []),
    enabledTab("automation"),
    ...(availability.hasPullRequest ? [enabledTab("pr")] : []),
    ...(availability.hasJira ? [enabledTab("jira")] : []),
    ...(availability.hasLinear ? [enabledTab("linear")] : []),
    ...(availability.hasGranola ? [enabledTab("granola")] : []),
    ...(availability.hasReview ? [enabledTab("review")] : []),
    ...(availability.hasPublishWorkspace ? [enabledTab("publish")] : []),
  ];
}

function defaultSetupTab(
  tabs: AutomationConversationPolicyTab[],
): AutomationConversationTabId {
  const enabled = tabs.find((tab) => tab.enabled);
  return enabled?.id ?? "automation";
}

function isJudgeSettlingStatus(
  runStatus: AutomationRunStatus | null,
  judgeState: AutomationJudgeState | null,
): boolean {
  return (
    (runStatus === "merged" ||
      runStatus === "pr_closed" ||
      runStatus === "completed") &&
    (judgeState === "none" || judgeState === "in_progress")
  );
}

function defaultRunTab(input: AutomationConversationTabPolicyInput): AutomationConversationTabId {
  const { runStatus, workspaceMode, availability, tabHint } = input;
  if (tabHint) return tabHint;
  if (runStatus === "awaiting_plan_approval") {
    return "plan";
  }
  if (runStatus === "running" && workspaceMode === "plan") {
    return "plan";
  }
  if (
    (runStatus === "published" || isJudgeSettlingStatus(runStatus, input.judgeState)) &&
    availability.hasPullRequest
  ) {
    return "pr";
  }
  return "automation";
}

function runTabs(
  availability: AutomationConversationTabAvailability,
): AutomationConversationPolicyTab[] {
  return [
    enabledTab("automation"),
    availability.hasPlanArtifact
      ? enabledTab("plan")
      : disabledTab("plan", "No run plan has been authored yet."),
    ...(availability.hasPullRequest ? [enabledTab("pr")] : []),
    ...(availability.hasPublishWorkspace ? [enabledTab("publish")] : []),
  ];
}

export function getAutomationConversationTabPolicy(
  input: AutomationConversationTabPolicyInput,
): AutomationConversationTabPolicy {
  if (input.surface === "run") {
    const tabs = runTabs(input.availability);
    return { tabs, defaultTab: defaultRunTab(input) };
  }
  const tabs = setupTabs(input.availability);
  return { tabs, defaultTab: defaultSetupTab(tabs) };
}
