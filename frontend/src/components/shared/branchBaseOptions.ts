import { chatApi, type AgentConversationSourcePullRequest } from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { planBranchApi } from "@/api/plan-branch";
import {
  getGitBranches,
  getGitCurrentBranch,
  getGitDefaultBranch,
  searchGithubPullRequests,
} from "@/api/projects";
import type { ChatConversation } from "@/types/chat-conversation";

export type BranchBaseRefKind = "project_default" | "current_branch" | "local_branch";

export interface BranchBaseSelection {
  kind: BranchBaseRefKind;
  ref: string;
  displayName: string;
  sourcePullRequest?: AgentConversationSourcePullRequest | null;
  retargetedFromPullRequest?: number | undefined;
}

export type BranchBaseOptionSource =
  | "project"
  | "current"
  | "local"
  | "plan"
  | "agent"
  | "pull_request";

export interface BranchBaseOption {
  key: string;
  label: string;
  detail?: string | undefined;
  source: BranchBaseOptionSource;
  selection: BranchBaseSelection;
}

export interface LoadBranchBaseOptionsInput {
  projectId?: string | null;
  workingDirectory: string;
  projectBaseBranch?: string | null | undefined;
  includePlanBranches?: boolean;
  includeAgentBranches?: boolean;
}

export interface LoadBranchBaseOptionsResult {
  options: BranchBaseOption[];
  selectedKey: string;
  /** Present sub-loads that failed. All-false means a complete list. */
  degraded: BranchBaseLoadDegradation;
  /** Every normalized git branch name observed, including `ralphx/*`. */
  knownBranchRefs: string[];
}

export interface BranchBaseLoadDegradation {
  planBranches: boolean;
  agentBranches: boolean;
}

export function normalizeGitBranchName(branch: string) {
  return branch.trim().replace(/^[*+]\s+/, "");
}

export function isRalphxInternalBranch(branch: string) {
  return branch.startsWith("ralphx/");
}

export function compareBranchNames(a: string, b: string) {
  const aSpecial = isRalphxInternalBranch(a);
  const bSpecial = isRalphxInternalBranch(b);
  if (aSpecial !== bSpecial) {
    return aSpecial ? 1 : -1;
  }
  return a.localeCompare(b, undefined, { sensitivity: "base" });
}

export function synthesizeLocalBranchOption(
  ref: string,
  label?: string
): BranchBaseOption {
  const displayName = label ?? ref;
  return {
    key: `local_branch:${ref}`,
    label: displayName,
    detail: "Local branch",
    source: "local",
    selection: {
      kind: "local_branch",
      ref,
      displayName,
    },
  };
}

export async function loadBranchBaseOptions({
  projectId,
  workingDirectory,
  projectBaseBranch,
  includePlanBranches = true,
  includeAgentBranches = true,
}: LoadBranchBaseOptionsInput): Promise<LoadBranchBaseOptionsResult> {
  const [defaultResult, currentResult, branchesResult, planOptionsResult, agentOptionsResult] =
    await Promise.allSettled([
      getGitDefaultBranch(workingDirectory),
      getGitCurrentBranch(workingDirectory),
      getGitBranches(workingDirectory),
      includePlanBranches && projectId
        ? loadPlanBranchOptions(projectId)
        : Promise.resolve({ options: [], failed: false }),
      includeAgentBranches && projectId
        ? loadAgentBranchOptions(projectId)
        : Promise.resolve({ options: [], failed: false }),
    ]);

  const configuredProjectBase = projectBaseBranch?.trim();
  const projectDefault = normalizeGitBranchName(
    configuredProjectBase ||
      (defaultResult.status === "fulfilled" && defaultResult.value
        ? defaultResult.value
        : "main")
  );
  const currentBranch = normalizeGitBranchName(
    currentResult.status === "fulfilled" && currentResult.value
      ? currentResult.value
      : projectDefault
  );
  const branches =
    branchesResult.status === "fulfilled" && Array.isArray(branchesResult.value)
      ? branchesResult.value.map(normalizeGitBranchName).filter(Boolean)
      : [projectDefault];
  const branchSet = new Set(branches);
  const planBranchLoad =
    planOptionsResult.status === "fulfilled"
      ? planOptionsResult.value
      : { options: [], failed: true };
  const agentBranchLoad =
    agentOptionsResult.status === "fulfilled"
      ? agentOptionsResult.value
      : { options: [], failed: true };
  const degraded = {
    planBranches: planOptionsResult.status === "rejected" || planBranchLoad.failed,
    agentBranches: agentOptionsResult.status === "rejected" || agentBranchLoad.failed,
  };

  const optionMap = new Map<string, BranchBaseOption>();
  const addOption = (option: BranchBaseOption) => {
    optionMap.set(option.key, option);
  };

  addOption({
    key: `project_default:${projectDefault}`,
    label: `Project default (${projectDefault})`,
    detail: "Configured project base branch",
    source: "project",
    selection: {
      kind: "project_default",
      ref: projectDefault,
      displayName: `Project default (${projectDefault})`,
    },
  });

  if (currentBranch && currentBranch !== projectDefault) {
    addOption({
      key: `current_branch:${currentBranch}`,
      label: `Current branch (${currentBranch})`,
      detail: "Currently checked out in the project root",
      source: "current",
      selection: {
        kind: "current_branch",
        ref: currentBranch,
        displayName: `Current branch (${currentBranch})`,
      },
    });
  }

  branches
    .filter(
      (branch) =>
        branch &&
        branch !== projectDefault &&
        branch !== currentBranch &&
        !isRalphxInternalBranch(branch)
    )
    .sort(compareBranchNames)
    .forEach((branch) => {
      addOption({
        key: `local_branch:${branch}`,
        label: branch,
        detail: "Local branch",
        source: "local",
        selection: {
          kind: "local_branch",
          ref: branch,
          displayName: branch,
        },
      });
    });

  const knownGeneratedOptions = [
    ...planBranchLoad.options,
    ...agentBranchLoad.options,
  ]
    .filter((option) => branchSet.has(option.selection.ref))
    .sort((a, b) => {
      const sourceRank = sourceSortRank(a.source) - sourceSortRank(b.source);
      return sourceRank || a.label.localeCompare(b.label, undefined, { sensitivity: "base" });
    });

  knownGeneratedOptions.forEach(addOption);

  return {
    options: Array.from(optionMap.values()),
    selectedKey: `project_default:${projectDefault}`,
    degraded,
    knownBranchRefs: branches,
  };
}

export function fallbackBranchBaseOptions(
  baseBranch: string | null | undefined
): LoadBranchBaseOptionsResult {
  const fallback = normalizeGitBranchName(baseBranch ?? "main");
  return {
    options: [
      {
        key: `project_default:${fallback}`,
        label: `Project default (${fallback})`,
        detail: "Configured project base branch",
        source: "project" as const,
        selection: {
          kind: "project_default" as const,
          ref: fallback,
          displayName: `Project default (${fallback})`,
        },
      },
    ],
    selectedKey: `project_default:${fallback}`,
    degraded: { planBranches: false, agentBranches: false },
    knownBranchRefs: [fallback],
  };
}

export async function loadPullRequestBaseOptions({
  projectId,
  query,
}: {
  projectId: string;
  query?: string;
}): Promise<BranchBaseOption[]> {
  const input = {
    projectId,
    limit: 50,
    ...(query !== undefined ? { query } : {}),
  };
  const pullRequests = await searchGithubPullRequests(input);
  const options: BranchBaseOption[] = [];

  for (const pullRequest of pullRequests) {
    if (pullRequest.isCrossRepository) {
      continue;
    }
    const branchName = normalizeGitBranchName(pullRequest.headRefName);
    if (!branchName) {
      continue;
    }
    const title = pullRequest.title.trim() || `Pull request #${pullRequest.number}`;
    const mergeTarget = normalizeGitBranchName(pullRequest.baseRefName);
    const state = pullRequest.state?.toUpperCase();
    const isMerged = state === "MERGED";
    const isClosed = state === "CLOSED";
    if (isClosed) {
      continue;
    }
    if (isMerged && !mergeTarget) {
      continue;
    }
    const draftLabel = pullRequest.isDraft ? " - Draft" : "";
    const mergedDetail = `Merged → ${mergeTarget}`;
    options.push({
      key: `pull_request:${pullRequest.number}:${branchName}`,
      label: `#${pullRequest.number} ${title}`,
      detail: isMerged ? mergedDetail : `${branchName} -> ${pullRequest.baseRefName}${draftLabel}`,
      source: "pull_request",
      selection: {
        kind: "local_branch",
        ref: isMerged ? mergeTarget : branchName,
        displayName: isMerged
          ? `${mergeTarget} (PR #${pullRequest.number} merged)`
          : `PR #${pullRequest.number}: ${title}`,
        ...(isMerged ? { retargetedFromPullRequest: pullRequest.number } : {}),
        ...(isMerged
          ? {}
          : {
              sourcePullRequest: {
                number: pullRequest.number,
                url: pullRequest.url ?? null,
                title,
                headRefName: branchName,
                baseRefName: pullRequest.baseRefName ?? null,
                headRefOid: pullRequest.headRefOid ?? null,
              },
            }),
      },
    });
  }

  return options.sort(
    (left, right) =>
      Number(left.selection.retargetedFromPullRequest !== undefined) -
      Number(right.selection.retargetedFromPullRequest !== undefined),
  );
}

async function loadPlanBranchOptions(
  projectId: string
): Promise<{ options: BranchBaseOption[]; failed: boolean }> {
  try {
    const [branches, sessions] = await Promise.all([
      planBranchApi.getByProject(projectId),
      ideationApi.sessions.list(projectId),
    ]);
    const titleBySessionId = new Map(
      sessions.map((session) => [session.id, session.title ?? `Plan ${session.id.slice(0, 8)}`])
    );

    return {
      options: branches
        .filter((branch) => branch.status === "active")
        .map((branch) => {
          const branchName = normalizeGitBranchName(branch.branchName);
          const title = titleBySessionId.get(branch.sessionId) ?? `Plan ${branch.sessionId.slice(0, 8)}`;
          return {
            key: `local_branch:${branchName}`,
            label: title,
            detail: branchName,
            source: "plan" as const,
            selection: {
              kind: "local_branch" as const,
              ref: branchName,
              displayName: title,
            },
          };
        }),
      failed: false,
    };
  } catch {
    return { options: [], failed: true };
  }
}

async function loadAgentBranchOptions(
  projectId: string
): Promise<{ options: BranchBaseOption[]; failed: boolean }> {
  try {
    const [conversations, workspaces] = await Promise.all([
      chatApi.listConversations("project", projectId, false),
      chatApi.listAgentConversationWorkspacesByProject(projectId),
    ]);
    const conversationById = new Map(
      conversations.map((conversation) => [conversation.id, conversation])
    );

    return {
      options: workspaces.flatMap((workspace) => {
        if (workspace.status === "missing" || workspace.linkedPlanBranchId) {
          return [];
        }

        const conversation = conversationById.get(workspace.conversationId);
        if (!conversation) {
          return [];
        }

        const branchName = normalizeGitBranchName(workspace.branchName);
        return [
          {
            key: `local_branch:${branchName}`,
            label: agentWorkspaceTitle(conversation),
            detail: branchName,
            source: "agent" as const,
            selection: {
              kind: "local_branch" as const,
              ref: branchName,
              displayName: agentWorkspaceTitle(conversation),
            },
          },
        ];
      }),
      failed: false,
    };
  } catch {
    return { options: [], failed: true };
  }
}

function agentWorkspaceTitle(conversation: ChatConversation) {
  const title = conversation.title?.trim();
  return title && title !== "Untitled agent"
    ? title
    : `Agent conversation ${conversation.id.slice(0, 8)}`;
}

function sourceSortRank(source: BranchBaseOptionSource) {
  switch (source) {
    case "plan":
      return 0;
    case "agent":
      return 1;
    default:
      return 2;
  }
}
