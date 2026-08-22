import type { Page } from "@playwright/test";
import { mockGuideWorkspaceFileDiffPage } from "@/api-mock/guide-diff-pages";
import type { GuideScenarioName } from "@/api-mock/guide-scenarios";

/** Seeds the app-owned guide registry and settles the queries that render it. */
export async function applyGuideScenario(
  page: Page,
  name: GuideScenarioName,
): Promise<void> {
  if (name === "guide_implementing") {
    await page.route(
      "**/api/agent-workspaces/**/file-diff-page**",
      async (route) => {
        const url = new URL(route.request().url());
        const path = url.searchParams.get("path") ?? "unknown";
        const offset = Number(url.searchParams.get("offset") ?? "0");
        const limit = Number(url.searchParams.get("limit") ?? "200");
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(mockGuideWorkspaceFileDiffPage(path, offset, limit)),
        });
      },
    );
  }
  await page.evaluate(async (scenarioName) => {
    const guide = await import("/src/api-mock/guide-scenarios.ts");
    const {
      mockGetAgentConversationWorkspace,
      mockGetConversation,
      mockStartAgentConversation,
      seedMockAgentConversationWorkspace,
    } = await import("/src/api-mock/chat");
    const { mockIdeationApi } = await import("/src/api-mock/ideation");
    window.__mockChatApi?.seedScenario(scenarioName);
    guide.seedGuideStore(scenarioName);
    if (scenarioName !== "guide_onboarding") {
      const { useProjectStore } = await import("/src/stores/projectStore");
      useProjectStore
        .getState()
        .setProjects(guide.GUIDE_SCENARIO_FIXTURES[scenarioName].projects);
      useProjectStore.getState().selectProject("guide-project");
    }
    const conversationId = `conversation-${scenarioName}`;
    const mode =
      scenarioName === "guide_planning"
        ? "plan"
        : scenarioName === "guide_pr_review"
          ? "review_pr"
          : "edit";
    if (scenarioName !== "guide_onboarding") {
      await mockStartAgentConversation({
        projectId: "guide-project",
        content: "Prepare the guide capture workspace.",
        conversationId,
        providerHarness: "codex",
        modelId: "gpt-5.4",
        mode,
        base: {
          kind: "project_default",
          ref: "main",
          displayName: "Project default (main)",
        },
      });
      const workspace = await mockGetAgentConversationWorkspace(conversationId);
      if (!workspace || !window.__queryClient)
        throw new Error("Guide workspace was not created");
      const now = "2026-06-15T10:00:00.000Z";
      const seededWorkspace =
        scenarioName === "guide_pr_review"
          ? {
              ...workspace,
              publicationPrNumber: 128,
              publicationPrUrl:
                "https://github.com/ralphx/release-companion/pull/128",
              publicationPrStatus: "open",
              publicationPushStatus: "pushed",
              prSupervisionStatus: "monitoring",
              prSupervisionSummary:
                "Monitoring the release readiness pull request.",
              prSupervisionUpdatedAt: now,
            }
          : workspace;
      seedMockAgentConversationWorkspace(seededWorkspace);
      window.__queryClient.setQueryData(
        ["agents", "conversation-workspace", conversationId],
        seededWorkspace,
      );
      window.__queryClient.setQueryData(
        ["chat", "conversations", conversationId],
        await mockGetConversation(conversationId),
      );
      await window.__queryClient.invalidateQueries();
      if (scenarioName === "guide_planning" || scenarioName === "guide_tour") {
        const sessionId = `${conversationId}-ideation-session`;
        const artifactId = `${conversationId}-plan-artifact`;
        mockIdeationApi.sessions.seedWithData({
          session: {
            id: sessionId,
            projectId: "guide-project",
            title: guide.GUIDE_RELEASE_PLAN_TITLE,
            titleSource: null,
            status: "active",
            planArtifactId: artifactId,
            seedTaskId: null,
            parentSessionId: null,
            createdAt: now,
            updatedAt: now,
            archivedAt: null,
            convertedAt: null,
            verificationStatus: "verified",
            verificationInProgress: false,
            gapScore: 0,
            sessionPurpose: "general",
            sessionFlow: "planning",
            acceptanceStatus: null,
          },
          proposals: [],
          messages: [],
        });
        const plan = {
          id: artifactId,
          type: "design_doc",
          name: "Release readiness overview",
          content: {
            type: "inline",
            text: [
              "# Release readiness workspace",
              "",
              "Coordinate validation, review, and a dependable handoff for every release.",
              "",
              "## Outcome",
              "",
              "Every release leaves RalphX with a named owner, a completed review gate, and a handoff note the on-call engineer can act on without asking questions.",
              "",
              "## Scope",
              "",
              "- Add a release checklist to the workspace with owner, validation, and rollback fields.",
              "- Require a workspace review pass before the branch can be published.",
              "- Record the rollback owner alongside the migration validation command.",
              "",
              "## Out of scope",
              "",
              "- Changing the deployment pipeline itself.",
              "- Notification or paging behavior.",
              "",
              "## Acceptance criteria",
              "",
              "- The checklist blocks publication until validation and handoff fields are filled in.",
              "- A release with a missing rollback owner is reported as blocking, not passing.",
              "- The handoff note is visible from the workspace without opening CI.",
            ].join("\n"),
          },
          metadata: { createdAt: now, createdBy: "RalphX", version: 1 },
          derivedFrom: [],
          bucketId: undefined,
          artifactRole: "overview",
          planContractVersion: 2,
          planApproval: {
            status: scenarioName === "guide_tour" ? "approved" : "draft",
          },
          blueprint: {
            id: "guide-release-readiness-blueprint",
            type: "design_doc",
            name: "Implementation Blueprint",
            content: {
              type: "inline",
              text: [
                "# Implementation Blueprint",
                "",
                "## 1. Add the release checklist fields",
                "",
                "Extend the workspace checklist with owner, validation command, and rollback owner.",
                "Validation: the checklist renders all three fields and rejects an empty rollback owner.",
                "",
                "## 2. Gate publication on workspace review",
                "",
                "Require a current workspace review result before the publish action is offered.",
                "Validation: a stale review result does not authorize publication.",
                "",
                "## 3. Surface the handoff note",
                "",
                "Render the completed checklist as the branch handoff summary.",
                "Validation: the summary is readable from the workspace without opening CI.",
              ].join("\n"),
            },
            metadata: { createdAt: now, createdBy: "RalphX", version: 1 },
            derivedFrom: [],
            bucketId: undefined,
            artifactRole: "blueprint",
          },
        };
        const linkedWorkspace = {
          ...seededWorkspace,
          linkedIdeationSessionId: sessionId,
          linkedPlanBranchId: null,
        };
        seedMockAgentConversationWorkspace(linkedWorkspace);
        window.__queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          linkedWorkspace,
        );
        window.__queryClient.setQueryData(
          ["agents", "artifact", artifactId],
          plan,
        );
        window.__queryClient.setQueryData(
          ["agents", "session-plan", sessionId, artifactId],
          plan,
        );
        window.__queryClient.setQueryData(
          ["agents", "plan-approval", sessionId],
          plan,
        );
        window.__queryClient.setQueryData(
          ["ideation", "sessions", "detail", sessionId, "with-data"],
          await mockIdeationApi.sessions.getWithData(sessionId),
        );
        window.__queryClient.setQueryData(["ideation", "settings"], {
          tasksEnabled: true,
          tasksFeatureState: "enabled",
          autoVerifyDraftPlans: true,
          autoVerifyPlans: false,
          requireAcceptForFinalize: false,
          requireVerificationForAccept: false,
          externalOverrides: {
            autoVerifyPlans: null,
            requireVerificationForAccept: null,
            requireAcceptForFinalize: null,
          },
        });
      }
      if (scenarioName === "guide_local_review") {
        const review = {
          success: true,
          workspace: seededWorkspace,
          events: [],
          target: {
            scope: "workspace_delta",
            baseRef: "main",
            baseSha: "release-base",
            headRef: seededWorkspace.branchName,
            headSha: "release-head",
            diffFingerprint: "release-readiness-diff",
            sourcePullRequestNumber: null,
          },
          monitor: {
            conversationId,
            projectId: "guide-project",
            status: "ready",
            reviewOutcome: "blocking",
            reviewGateStatus: "blocking",
            currentTargetScope: "workspace_delta",
            reviewedTargetScope: "workspace_delta",
            reviewConversationId: null,
            reviewArtifactId: "guide-release-review",
            reviewRequestedChangesArtifactId:
              "guide-release-requested-changes",
            reviewArtifactVersion: 1,
            reviewArtifactUpdatedAt: now,
            reviewGateBypassedAt: null,
            reviewGateBypassedTargetScope: null,
            reviewGateBypassedDiffFingerprint: null,
            reviewGateBypassedArtifactId: null,
            reviewGateBypassedArtifactVersion: null,
            reviewedHeadSha: "release-head",
            reviewedDiffFingerprint: "release-readiness-diff",
            selectedSourceBaseRef: null,
            selectedSourceBaseSha: null,
            selectedSourceHeadRef: null,
            selectedSourceHeadSha: null,
            selectedSourcePullRequestNumber: null,
            workspaceBaseRef: "main",
            workspaceBaseSha: "release-base",
            workspaceHeadRef: seededWorkspace.branchName,
            workspaceHeadSha: "release-head",
            currentDiffFingerprint: "release-readiness-diff",
            previousVersionId: null,
            reviewBlockingSummary:
              "Two release-safety checks need attention before publishing.",
            reviewBlockingFingerprint: "release-review-findings",
            reviewFixerRunId: null,
            reviewFixerConversationId: null,
            reviewFixerStatus: null,
            reviewFixerCycleCount: 0,
            lastRunId: "guide-review-run",
            lastError: null,
            autoMergeGuardStatus: null,
            autoMergeGuardPrNumber: null,
            autoMergeGuardMethod: null,
            autoMergeGuardTargetScope: null,
            autoMergeGuardDiffFingerprint: null,
            autoMergeGuardHeadSha: null,
            autoMergeGuardLastError: null,
            createdAt: now,
            updatedAt: now,
          },
          reviewArtifactIsCurrent: true,
          reviewArtifactIsOutdated: false,
          canMutateReviewState: true,
          reviewRuntimeState: "terminal",
          isCurrent: true,
          isOutdated: false,
          shouldShowTab: true,
        };
        window.__queryClient.setQueryData(
          ["agents", "workspace-review-context", conversationId],
          review,
        );
        window.__queryClient.setQueryData(
          ["agents", "workspace-review", conversationId],
          {
            changes: [
              {
                path: "docs/release-readiness.md",
                status: "modified",
                additions: 12,
                deletions: 2,
                isGenerated: false,
              },
            ],
            commits: [],
            baseRef: "main",
            headRef: seededWorkspace.branchName,
            supportsWorktreeModes: true,
          },
        );
        window.__queryClient.setQueryData(
          ["agents", "workspace-change-summary", conversationId],
          {
            supportsWorktreeModes: true,
            staged: { fileCount: 0, additions: 0, deletions: 0 },
            unstaged: { fileCount: 1, additions: 12, deletions: 2 },
            conflicted: { fileCount: 0, files: [] },
          },
        );
        window.__queryClient.setQueryData(
          ["agents", "artifact", "guide-release-review"],
          {
            id: "guide-release-review",
            type: "workspace_review",
            name: "Release readiness review",
            content: {
              type: "inline",
          text: "# Overview\n\nThe release is close to ready. The workspace review found two concrete safeguards to resolve before publishing.\n\n## What is working\n\n- The release checklist now covers validation and handoff.\n- The workspace branch is ready for a final review pass.\n\n# Requested Changes\n\n- Add the final rollback owner to the checklist.\n- Confirm the migration validation command in CI.",
            },
            metadata: {
              createdAt: now,
              createdBy: "RalphX Workspace Review",
              version: 1,
            },
            derivedFrom: [],
            bucketId: undefined,
          },
        );
        window.__queryClient.setQueryData(
          ["agents", "artifact", "guide-release-requested-changes"],
          {
            id: "guide-release-requested-changes",
            type: "workspace_review",
            name: "Requested Changes",
            content: {
              type: "inline",
              text: "# Requested Changes\n\n- Add the final rollback owner to the checklist.\n- Confirm the migration validation command in CI.",
            },
            metadata: {
              createdAt: now,
              createdBy: "RalphX Workspace Review",
              version: 1,
            },
            derivedFrom: [],
            bucketId: undefined,
          },
        );
      }
      if (scenarioName === "guide_pr_review") {
        const pendingAction = {
          id: "guide-pr-action",
          conversationId,
          prNumber: 128,
          headSha: "release-head",
          proposedAction: "approve",
          summary:
            "All required checks passed and the release checklist is complete.",
          reviewBody:
            "Approved after confirming the release readiness checklist and workspace review findings.",
          findingsJson: null,
          status: "pending",
          submittedReviewId: null,
          createdByRunId: "guide-pr-review-run",
          createdAt: now,
          updatedAt: now,
          resolvedAt: null,
        };
        window.__queryClient.setQueryData(
          ["agents", "workspace-pr-review", conversationId],
          {
            success: true,
            workspace: seededWorkspace,
            events: [],
            prNumber: 128,
            prUrl: "https://github.com/ralphx/release-companion/pull/128",
            currentHeadSha: "release-head",
            pendingActionHeadStatus: "current",
            health: null,
            reviewFeedback: null,
            monitor: {
              conversationId,
              projectId: "guide-project",
              prNumber: 128,
              status: "awaiting_user",
              monitorEnabled: true,
              autoApproveEnabled: false,
              firstReviewCompleted: true,
              firstActionResolved: false,
              lastSeenHeadSha: "release-head",
              lastReviewedHeadSha: "release-head",
              lastReviewRunId: "guide-pr-review-run",
              lastReviewOutcome: "approved",
              lastSubmittedReviewId: null,
              reviewArtifactId: null,
              reviewArtifactHeadSha: "release-head",
              reviewArtifactVersion: null,
              reviewArtifactUpdatedAt: now,
              lastError: null,
              createdAt: now,
              updatedAt: now,
            },
            pendingAction,
            recentActions: [],
            issueCommentEvidence: [],
          },
        );
      }
      window.__chatStore
        ?.getState()
        .setActiveConversation(`project:guide-project`, conversationId);
      await window.__queryClient.invalidateQueries({
        queryKey: ["agents", "sidebar-conversations"],
      });
    }
    const conversations = window.__mockChatApi
      ?.listScenarios()
      .includes(scenarioName);
    if (!conversations)
      throw new Error(`Guide scenario ${scenarioName} did not register`);
  }, name);
}

/**
 * Puts the app in the genuine first-run state the install guide describes: no
 * configured agent harness and no projects, so the welcome screen shows the
 * Provider step as current with its **Set Up Provider** action.
 */
export async function applyFirstRunOnboarding(page: Page): Promise<void> {
  await page.evaluate(async () => {
    window.__mockProviderRequiresOnboarding = true;
    const { getStore } = await import("/src/api-mock/store");
    getStore().projects.clear();
    const { useProjectStore } = await import("/src/stores/projectStore");
    useProjectStore.getState().setProjects([]);
    await window.__queryClient?.invalidateQueries();
  });
}

/** Rehydrates the Agents plan cache after its workspace queries have mounted. */
export async function hydrateGuidePlanningArtifactCache(
  page: Page,
  conversationId: string,
): Promise<void> {
  await page.evaluate(async (targetConversationId) => {
    const queryClient = window.__queryClient;
    if (!queryClient) throw new Error("Expected guide query client");

    const { mockIdeationApi } = await import("/src/api-mock/ideation");
    const sessionId = `${targetConversationId}-ideation-session`;
    const sessionData = await mockIdeationApi.sessions.getWithData(sessionId);
    const planArtifactId = sessionData?.session.planArtifactId;
    const planArtifact = planArtifactId
      ? queryClient.getQueryData(["agents", "artifact", planArtifactId])
      : null;

    queryClient.setQueryData(
      ["ideation", "sessions", "detail", sessionId, "with-data"],
      sessionData,
    );
    if (planArtifactId && planArtifact) {
      queryClient.setQueryData(
        ["agents", "session-plan", sessionId, planArtifactId],
        planArtifact,
      );
    }
  }, conversationId);
}

/** Restores the review documents after the publish pane mounts its artifact queries. */
export async function hydrateGuideLocalReviewArtifactCache(
  page: Page,
): Promise<void> {
  await page.evaluate(() => {
    const queryClient = window.__queryClient;
    if (!queryClient) throw new Error("Expected guide query client");
    const now = "2026-06-15T10:00:00.000Z";
    const contextKey = [
      "agents",
      "workspace-review-context",
      "conversation-guide_local_review",
    ];
    const context = queryClient.getQueryData<Record<string, unknown>>(contextKey);
    if (context) {
      const monitor = context.monitor as Record<string, unknown>;
      queryClient.setQueryData(contextKey, {
        ...context,
        monitor: {
          ...monitor,
          reviewArtifactId: "guide-release-review",
          reviewRequestedChangesArtifactId: "guide-release-requested-changes",
        },
      });
    }
    queryClient.setQueryData(["agents", "artifact", "guide-release-review"], {
      id: "guide-release-review",
      type: "workspace_review",
      name: "Release readiness review",
      content: {
        type: "inline",
        text: "# Overview\n\nThe release is close to ready. The workspace review found two concrete safeguards to resolve before publishing.\n\n## What is working\n\n- The release checklist now covers validation and handoff.\n- The workspace branch is ready for a final review pass.",
      },
      metadata: { createdAt: now, createdBy: "RalphX Workspace Review", version: 1 },
      derivedFrom: [],
      bucketId: undefined,
    });
    queryClient.setQueryData(
      ["agents", "artifact", "guide-release-requested-changes"],
      {
        id: "guide-release-requested-changes",
        type: "workspace_review",
        name: "Requested Changes",
        content: {
          type: "inline",
          text: "# Requested Changes\n\n- Add the final rollback owner to the checklist.\n- Confirm the migration validation command in CI.",
        },
        metadata: { createdAt: now, createdBy: "RalphX Workspace Review", version: 1 },
        derivedFrom: [],
        bucketId: undefined,
      },
    );
  });
}
