import type { MockChatScenario } from "./chat-scenarios";
import type { ChatMessageResponse } from "@/api/chat";
import { createMockProject, createMockTask } from "@/test/mock-data";
import type { Project } from "@/types/project";
import type { Task } from "@/types/task";
import type { TaskStep } from "@/types/task-step";
import { getStore } from "./store";

export type GuideScenarioName =
  | "guide_onboarding"
  | "guide_tour"
  | "guide_planning"
  | "guide_implementing"
  | "guide_local_review"
  | "guide_pr_review"
  | "guide_github_settings"
  | "guide_settings_atlassian"
  | "guide_settings_providers"
  | "guide_providers_cli_not_ready"
  | "guide_settings_capacity";

/** Expected shipped values from config/ralphx.yaml:38-49. Keep hand-written. */
export const PROD_UI_FEATURE_FLAGS = {
  activityPage: false,
  extensibilityPage: false,
  automationsPage: true,
  atlassianOauth: false,
  ticketingDashboard: false,
  agentPersonas: false,
  standaloneConversations: false,
  personaSwitchForcesFreshProviderSession: false,
};

type GuideScenario = MockChatScenario & {
  projects: Project[];
  tasks: Task[];
  taskSteps: Record<string, TaskStep[]>;
};

const timestamp = "2026-06-15T10:00:00.000Z";
export const GUIDE_RELEASE_PLAN_TITLE = "Release readiness workspace";
const project = createMockProject({
  id: "guide-project",
  name: "RalphX Release Companion",
  workingDirectory: "/work/ralphx-release-companion",
  githubPrEnabled: true,
  // Guides describe the connected-GitHub happy path, so the publish surface has
  // to resolve to `newPr` instead of the local-only "cannot open GitHub pull
  // requests" fallback.
  repositoryCapability: {
    kind: "github",
    fetchUrl: "https://github.com/ralphx/release-companion.git",
    pushUrl: "https://github.com/ralphx/release-companion.git",
  },
});

function conversation(
  name: GuideScenarioName,
  contextType: "project" | "task_execution" | "review" = "project",
) {
  const contextId = contextType === "project" ? project.id : "guide-task";
  const id = `conversation-${name}`;
  return {
    id,
    contextType,
    contextId,
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: "claude_code" as const,
    coordinationMode: "solo" as const,
    title: GUIDE_RELEASE_PLAN_TITLE,
    messageCount: name === "guide_implementing" ? 3 : 2,
    lastMessageAt: timestamp,
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

function chatFixture(
  name: GuideScenarioName,
  contextType: "project" | "task_execution" | "review" = "project",
): MockChatScenario {
  const item = conversation(name, contextType);
  const implementationMessages: ChatMessageResponse[] =
    name === "guide_implementing"
      ? [
          {
            id: `${item.id}-activity`,
            sessionId: null,
            projectId: project.id,
            taskId: "guide-task",
            role: "assistant" as const,
            content: "",
            metadata: null,
            parentMessageId: `${item.id}-request`,
            conversationId: item.id,
            toolCalls: null,
            contentBlocks: [
              {
                type: "text",
                text: "I’m implementing the release checklist and validating the publish path.",
              },
              {
                type: "tool_use",
                id: `${item.id}-inspect`,
                name: "functions.exec_command",
                arguments: { cmd: "rg -n \"release checklist\" frontend src-tauri" },
                result: "frontend/src/components/agents/ReleaseChecklist.tsx:42: export function ReleaseChecklist()",
              },
              {
                type: "tool_use",
                id: `${item.id}-edit`,
                name: "functions.apply_patch",
                arguments: { patch: "*** Update File: ReleaseChecklist.tsx\n+ add workspace review gate" },
                result: "Done!",
              },
              {
                type: "tool_use",
                id: `${item.id}-validate`,
                name: "functions.exec_command",
                arguments: { cmd: "npm run typecheck" },
                result: "Typecheck passed.",
              },
            ],
            sender: "orchestrator",
            createdAt: timestamp,
          },
        ]
      : [];
  return {
    conversations: [item],
    messages: {
      [item.id]: [
        {
          id: `${item.id}-request`,
          sessionId: null,
          projectId: project.id,
          taskId: contextType === "project" ? null : "guide-task",
          role: "user",
          content:
            "Prepare a clear, dependable release readiness workflow for the team.",
          metadata: null,
          parentMessageId: null,
          conversationId: item.id,
          toolCalls: null,
          contentBlocks: null,
          sender: null,
          createdAt: timestamp,
        },
        {
          id: `${item.id}-answer`,
          sessionId: null,
          projectId: project.id,
          taskId: contextType === "project" ? null : "guide-task",
          role: "assistant",
          content:
            "The release path is mapped, review points are visible, and the next action is ready for the team.",
          metadata: null,
          parentMessageId: `${item.id}-request`,
          conversationId: item.id,
          toolCalls: null,
          contentBlocks: null,
          sender: "orchestrator",
          createdAt: timestamp,
        },
        ...implementationMessages,
      ],
    },
  };
}

function scenario(
  name: GuideScenarioName,
  status: Task["internalStatus"],
  contextType: "project" | "task_execution" | "review" = "project",
): GuideScenario {
  const chat = chatFixture(name, contextType);
  const task = createMockTask({
    id: "guide-task",
    projectId: project.id,
    title: "Ship a dependable release checklist",
    description:
      "Give every release a clear owner, a review gate, and a reliable handoff.",
    internalStatus: status,
    taskBranch: "ralphx/release-checklist",
    worktreePath: "/work/ralphx-release-companion",
    planArtifactId: "guide-plan",
  });
  return {
    ...chat,
    projects: [project],
    tasks: [task],
    taskSteps: { [task.id]: [] },
  };
}

const onboarding: GuideScenario = {
  conversations: [],
  messages: {},
  projects: [],
  tasks: [],
  taskSteps: {},
};



export const GUIDE_SCENARIO_FIXTURES: Record<GuideScenarioName, GuideScenario> =
  {
    guide_onboarding: onboarding,
    guide_tour: scenario("guide_tour", "approved"),
    guide_planning: scenario("guide_planning", "ready"),
    guide_implementing: scenario("guide_implementing", "executing"),
    guide_local_review: scenario("guide_local_review", "revision_needed"),
    guide_pr_review: scenario("guide_pr_review", "reviewing"),
    guide_github_settings: scenario("guide_github_settings", "approved"),
    guide_settings_atlassian: scenario("guide_settings_atlassian", "approved"),
    guide_settings_providers: scenario("guide_settings_providers", "approved"),
    guide_providers_cli_not_ready: scenario("guide_providers_cli_not_ready", "approved"),
    guide_settings_capacity: scenario("guide_settings_capacity", "approved"),
  };

/**
 * Atlassian fixtures for the Jira/Confluence guide captures.
 *
 * The default `tauri-api-core` Jira mocks return placeholder text ("Mock issue
 * for ..."), which reads as broken UI in a published screenshot. These give the
 * Jira guide a realistic ticket whose fields match what the guide tells the
 * reader to look for.
 */
export const GUIDE_JIRA_SEARCH_RESULTS = [
  {
    kind: "jira",
    id: "10412",
    key: "REL-214",
    title: "Block publishing until the release checklist is complete",
    url: "https://acme.atlassian.net/browse/REL-214",
    excerpt:
      "Releases are going out without a named rollback owner. Gate publication on the checklist.",
  },
  {
    kind: "jira",
    id: "10408",
    key: "REL-209",
    title: "Record the rollback owner next to the migration command",
    url: "https://acme.atlassian.net/browse/REL-209",
    excerpt:
      "On-call has to guess who owns a rollback when a migration fails after hours.",
  },
  {
    kind: "jira",
    id: "10395",
    key: "REL-198",
    title: "Surface the release handoff note outside CI",
    url: "https://acme.atlassian.net/browse/REL-198",
    excerpt:
      "The handoff summary is only reachable from a CI log, so nobody reads it.",
  },
] as const;

export const GUIDE_JIRA_ISSUE = {
  projectId: "guide-project",
  provider: "atlassian",
  issueKey: "REL-214",
  issueId: "10412",
  issueUrl: "https://acme.atlassian.net/browse/REL-214",
  title: "Block publishing until the release checklist is complete",
  status: "In Progress",
  assignee: null,
  reporter: "Dana Whitfield",
  updatedAtRemote: "2026-06-15T08:42:00.000Z",
  descriptionMarkdown: [
    "Releases are reaching production without a named rollback owner, so the on-call",
    "engineer has to page the author to find out who can revert a migration.",
    "",
    "Publication should stay blocked until the release checklist names an owner, a",
    "validation command, and a rollback owner.",
  ].join("\n"),
  descriptionText: null,
  acceptanceCriteriaMarkdown: [
    "- Publishing is blocked while the rollback owner is empty.",
    "- A release missing its validation command is reported as blocking, not passing.",
    "- The handoff note is readable from the workspace without opening CI.",
  ].join("\n"),
  acceptanceCriteriaText: null,
  comments: [
    {
      id: "c-1",
      author: "Priya Raman",
      createdAt: "2026-06-14T16:20:00.000Z",
      updatedAt: "2026-06-14T16:20:00.000Z",
      bodyMarkdown:
        "We agreed in the release sync to treat a missing rollback owner as blocking rather than a warning.",
      bodyText:
        "We agreed in the release sync to treat a missing rollback owner as blocking rather than a warning.",
    },
    {
      id: "c-2",
      author: "Dana Whitfield",
      createdAt: "2026-06-15T08:42:00.000Z",
      updatedAt: "2026-06-15T08:42:00.000Z",
      bodyMarkdown:
        "Checklist wording is in the attached doc — please keep the field names identical.",
      bodyText:
        "Checklist wording is in the attached doc — please keep the field names identical.",
    },
  ],
  attachments: [
    {
      id: "a-1",
      filename: "release-checklist-wording.pdf",
      size: 184_320,
      contentUrl: "https://acme.atlassian.net/secure/attachment/a-1",
    },
  ],
  lastRefreshedAt: "2026-06-15T10:00:00.000Z",
  refreshStatus: "loaded",
  refreshError: null,
  assignedAt: "2026-06-15T09:58:00.000Z",
  assignedFromMessageId: null,
  manuallyAssigned: false,
  createdAt: "2026-06-15T09:58:00.000Z",
  updatedAt: "2026-06-15T10:00:00.000Z",
} as const;

/** Replaces durable project/task state without changing the regression mock fixtures. */
export function seedGuideStore(name: GuideScenarioName): void {
  const fixture = GUIDE_SCENARIO_FIXTURES[name];
  const store = getStore();
  store.projects.clear();
  store.tasks.clear();
  store.taskSteps.clear();
  fixture.projects.forEach((item) => store.projects.set(item.id, { ...item }));
  fixture.tasks.forEach((item) => store.tasks.set(item.id, { ...item }));
  Object.entries(fixture.taskSteps).forEach(([taskId, steps]) =>
    store.taskSteps.set(
      taskId,
      steps.map((step) => ({ ...step })),
    ),
  );
}
