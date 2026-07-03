import { chatApi, type AgentConversationSourcePullRequest } from "@/api/chat";
import type { ComposerIntegrationReference } from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { planBranchApi } from "@/api/plan-branch";
import {
  getGitBranches,
  getGitCurrentBranch,
  getGitDefaultBranch,
  searchGithubPullRequests,
} from "@/api/projects";
import type { TicketAssociationItem } from "@/api/ticketing";
import type { ChatConversation } from "@/types/chat-conversation";

export type BranchBaseRefKind = "project_default" | "current_branch" | "local_branch";

export interface BranchBaseSelection {
  kind: BranchBaseRefKind;
  ref: string;
  displayName: string;
  sourcePullRequest?: AgentConversationSourcePullRequest | null;
}

export type BranchBaseOptionSource =
  | "project"
  | "current"
  | "local"
  | "plan"
  | "agent"
  | "pull_request";
export type TicketComposerProvider = "jira" | "linear" | "clickup";

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
        : Promise.resolve([]),
      includeAgentBranches && projectId
        ? loadAgentBranchOptions(projectId)
        : Promise.resolve([]),
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
    ...(planOptionsResult.status === "fulfilled" ? planOptionsResult.value : []),
    ...(agentOptionsResult.status === "fulfilled" ? agentOptionsResult.value : []),
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
  };
}

export function fallbackBranchBaseOptions(baseBranch: string | null | undefined) {
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
  };
}

export function ticketAssociationBranchBaseOption(
  association: TicketAssociationItem
): BranchBaseOption | null {
  const branchName = normalizeGitBranchName(
    association.branchName ?? association.subtitle ?? ""
  );
  if (!branchName) {
    return null;
  }

  const prNumber = association.prNumber ?? null;
  if (typeof prNumber === "number" && Number.isFinite(prNumber)) {
    const title = association.title.trim() || `PR #${prNumber}`;
    const baseRefName = association.baseRef?.trim() || null;
    return {
      key: `pull_request:${prNumber}:${branchName}`,
      label: title,
      detail: baseRefName ? `${branchName} -> ${baseRefName}` : branchName,
      source: "pull_request",
      selection: {
        kind: "local_branch",
        ref: branchName,
        displayName: title,
        sourcePullRequest: {
          number: prNumber,
          url: association.prUrl ?? null,
          title,
          headRefName: branchName,
          baseRefName,
          headRefOid: null,
        },
      },
    };
  }

  const title = association.title.trim() || branchName;
  return {
    key: `ticket_branch:${branchName}`,
    label: title,
    detail: branchName,
    source: "local",
    selection: {
      kind: "local_branch",
      ref: branchName,
      displayName: title,
    },
  };
}

export function ticketCanonicalBranchBaseOption(
  reference: ComposerIntegrationReference
): BranchBaseOption | null {
  const ticket = ticketBaseReferenceFromComposerReference(reference);
  if (!ticket) {
    return null;
  }
  const branchName = `ralphx/ticket/${ticket.provider}-${ticket.issueSlug}`;
  const title = `Ticket ${ticket.issueKey}`;
  return {
    key: `ticket_branch:${branchName}`,
    label: title,
    detail: branchName,
    source: "local",
    selection: {
      kind: "local_branch",
      ref: branchName,
      displayName: `${title} (${branchName})`,
    },
  };
}

function ticketBaseReferenceFromComposerReference(
  reference: ComposerIntegrationReference
): { provider: string; issueKey: string; issueSlug: string } | null {
  const provider = ticketProviderForComposerReference(reference);
  if (!provider) {
    return null;
  }
  const issueKey = (reference.key?.trim() || reference.id.trim());
  const issueSlug = sanitizeTicketBranchComponent(issueKey);
  if (!issueKey || !issueSlug) {
    return null;
  }
  return {
    provider,
    issueKey,
    issueSlug,
  };
}

export function ticketProviderForComposerReference(
  reference: ComposerIntegrationReference
): TicketComposerProvider | null {
  const provider = reference.provider.trim().toLowerCase();
  const kind = reference.kind.trim().toLowerCase();
  if ((provider === "atlassian" || provider === "jira") && kind === "jira") {
    return "jira";
  }
  if (provider === "linear" && kind === "linear") {
    return "linear";
  }
  if (provider === "clickup" && kind === "clickup") {
    return "clickup";
  }
  return null;
}

function sanitizeTicketBranchComponent(value: string): string | null {
  let output = "";
  let lastWasDash = false;
  for (const character of value) {
    const lower = character.toLowerCase();
    if ((lower >= "a" && lower <= "z") || (lower >= "0" && lower <= "9")) {
      output += lower;
      lastWasDash = false;
    } else if (!lastWasDash) {
      output += "-";
      lastWasDash = true;
    }
  }
  const trimmed = output.replace(/^-+|-+$/g, "");
  return trimmed.length > 0 ? trimmed : null;
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
    limit: 30,
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
    const draftLabel = pullRequest.isDraft ? " - Draft" : "";
    options.push({
      key: `pull_request:${pullRequest.number}:${branchName}`,
      label: `#${pullRequest.number} ${title}`,
      detail: `${branchName} -> ${pullRequest.baseRefName}${draftLabel}`,
      source: "pull_request",
      selection: {
        kind: "local_branch",
        ref: branchName,
        displayName: `PR #${pullRequest.number}: ${title}`,
        sourcePullRequest: {
          number: pullRequest.number,
          url: pullRequest.url ?? null,
          title,
          headRefName: branchName,
          baseRefName: pullRequest.baseRefName ?? null,
          headRefOid: pullRequest.headRefOid ?? null,
        },
      },
    });
  }

  return options;
}

async function loadPlanBranchOptions(projectId: string): Promise<BranchBaseOption[]> {
  try {
    const [branches, sessions] = await Promise.all([
      planBranchApi.getByProject(projectId),
      ideationApi.sessions.list(projectId),
    ]);
    const titleBySessionId = new Map(
      sessions.map((session) => [session.id, session.title ?? `Plan ${session.id.slice(0, 8)}`])
    );

    return branches
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
      });
  } catch {
    return [];
  }
}

async function loadAgentBranchOptions(projectId: string): Promise<BranchBaseOption[]> {
  try {
    const [conversations, workspaces] = await Promise.all([
      chatApi.listConversations("project", projectId, false),
      chatApi.listAgentConversationWorkspacesByProject(projectId),
    ]);
    const conversationById = new Map(
      conversations.map((conversation) => [conversation.id, conversation])
    );

    return workspaces.flatMap((workspace) => {
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
    });
  } catch {
    return [];
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
