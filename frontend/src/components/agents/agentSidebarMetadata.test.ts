import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";
import type { Project } from "@/types/project";
import type { AgentConversation } from "./agentConversations";
import {
  getConversationRefDisplay,
  getSidebarPublicationGroupLabel,
  getSidebarPublicationLabel,
  getSidebarPublicationLabelForWorkspace,
  getSidebarPublicationState,
  shouldShowConversationForPublicationFilters,
} from "./agentSidebarMetadata";

const workspace = (
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "edit",
  baseRefKind: "project_default",
  baseRef: "main",
  baseDisplayName: "Project default (main)",
  baseCommit: null,
  branchName: "ralphx/demo/agent-conversation-1",
  worktreePath: "/tmp/ralphx/conversation-1",
  linkedIdeationSessionId: null,
  linkedPlanBranchId: null,
  publicationPrNumber: null,
  publicationPrUrl: null,
  publicationPrStatus: null,
  publicationPushStatus: null,
  status: "active",
  createdAt: "2026-05-01T10:00:00Z",
  updatedAt: "2026-05-01T10:00:00Z",
  ...overrides,
});

const project = (overrides: Partial<Project> = {}): Project => ({
  id: "project-1",
  name: "RalphX",
  workingDirectory: "/tmp/ralphx",
  gitMode: "worktree",
  baseBranch: "main",
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: false,
  createdAt: "2026-05-01T10:00:00Z",
  updatedAt: "2026-05-01T10:00:00Z",
  ...overrides,
});

const conversation = (
  overrides: Partial<AgentConversation> = {}
): AgentConversation => ({
  id: "conversation-1",
  contextType: "project",
  contextId: "project-1",
  claudeSessionId: null,
  providerSessionId: "thread-1",
  providerHarness: "codex",
  upstreamProvider: null,
  providerProfile: null,
  title: "Fix sidebar",
  messageCount: 1,
  lastMessageAt: "2026-05-01T11:00:00Z",
  createdAt: "2026-05-01T10:00:00Z",
  updatedAt: "2026-05-01T11:00:00Z",
  archivedAt: null,
  projectId: "project-1",
  ideationSessionId: null,
  ...overrides,
});

describe("agent sidebar metadata helpers", () => {
  it("normalizes publication states and labels", () => {
    expect(getSidebarPublicationState(undefined)).toBe("active");
    expect(
      getSidebarPublicationState(workspace({ publicationPrStatus: " MERGED " }))
    ).toBe("merged");
    expect(getSidebarPublicationState(workspace({ publicationPrStatus: "closed" }))).toBe(
      "closed"
    );
    expect(
      getSidebarPublicationState(workspace({ publicationPushStatus: "needs_agent" }))
    ).toBe("uncommitted");
    expect(getSidebarPublicationState(workspace({ publicationPushStatus: "failed" }))).toBe(
      "unpushed"
    );
    expect(
      getSidebarPublicationState(workspace({ publicationPushStatus: "description_failed" }))
    ).toBe("unpushed");
    expect(getSidebarPublicationState(workspace({ publicationPrStatus: "draft" }))).toBe(
      "draft"
    );

    expect(getSidebarPublicationLabel("active")).toBeNull();
    expect(getSidebarPublicationLabel("uncommitted")).toBe("uncommitted");
    expect(getSidebarPublicationGroupLabel("merged")).toBe("Merged");
    expect(getSidebarPublicationGroupLabel("unknown" as never)).toBe("Active");
  });

  it("surfaces PR supervision attention labels without changing filter state", () => {
    expect(
      getSidebarPublicationLabelForWorkspace(
        workspace({
          publicationPushStatus: "needs_agent",
          prSupervisionStatus: "fixing",
        })
      )
    ).toBe("fixing");
    expect(
      getSidebarPublicationLabelForWorkspace(
        workspace({
          publicationPushStatus: "pushed",
          prSupervisionStatus: "monitoring",
          prAutoMergeCurrent: true,
        })
      )
    ).toBe("auto-merge");
    expect(
      getSidebarPublicationLabelForWorkspace(
        workspace({
          publicationPushStatus: "needs_agent",
          prSupervisionStatus: "held",
        }),
      ),
    ).toBe("paused");
    expect(
      getSidebarPublicationLabelForWorkspace(
        workspace({
          publicationPushStatus: "needs_agent",
          prSupervisionStatus: "paused",
        }),
      ),
    ).toBe("paused");
    expect(
      getSidebarPublicationState(
        workspace({
          publicationPushStatus: "pushed",
          prSupervisionStatus: "monitoring",
          prAutoMergeCurrent: true,
        })
      )
    ).toBe("active");
  });

  it("chooses PR metadata before branch fallback labels", () => {
    expect(
      getConversationRefDisplay(
        conversation(),
        project(),
        workspace({ publicationPrNumber: 123 })
      )
    ).toEqual({ kind: "pull-request", label: "PR #123" });

    expect(
      getConversationRefDisplay(conversation(), project(), workspace({ baseRef: "develop" }))
    ).toEqual({ kind: "branch", label: "develop" });

    expect(
      getConversationRefDisplay(
        conversation(),
        project({ baseBranch: "trunk" }),
        workspace({ baseRef: null, baseDisplayName: "Project default" })
      )
    ).toEqual({ kind: "branch", label: "Project default" });

    expect(
      getConversationRefDisplay(
        conversation(),
        project({ baseBranch: "trunk" }),
        workspace({ baseRef: null, baseDisplayName: null })
      )
    ).toEqual({ kind: "branch", label: "trunk" });

    expect(
      getConversationRefDisplay(
        conversation({ contextType: "task" }),
        project({ baseBranch: null }),
        workspace({ baseRef: null, baseDisplayName: null })
      )
    ).toEqual({ kind: "branch", label: "base" });
  });

  it("applies publication-state filters", () => {
    expect(shouldShowConversationForPublicationFilters(workspace(), [])).toBe(false);
    expect(
      shouldShowConversationForPublicationFilters(workspace({ publicationPrStatus: "merged" }), [
        "merged",
      ])
    ).toBe(true);
    expect(
      shouldShowConversationForPublicationFilters(workspace({ publicationPrStatus: "closed" }), [
        "merged",
      ])
    ).toBe(false);
  });
});
