import { describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
  AgentWorkspaceMaintenanceOperationHoldReason,
} from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  AgentsPublishHoldCard,
  selectHoldDrawerTimeline,
} from "./AgentsPublishHoldCard";

const { listAgentConversationWorkspacePublicationEventsMock } = vi.hoisted(() => ({
  listAgentConversationWorkspacePublicationEventsMock: vi.fn(),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      listAgentConversationWorkspacePublicationEvents: (...args: unknown[]) =>
        listAgentConversationWorkspacePublicationEventsMock(...args),
    },
  };
});

const baseWorkspace: AgentConversationWorkspace = {
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "edit",
  branchMode: "isolated",
  baseRefKind: "project_default",
  baseRef: "main",
  baseDisplayName: "main",
  baseCommit: null,
  branchName: "ralphx/held",
  worktreePath: "/tmp/held",
  linkedIdeationSessionId: null,
  linkedPlanBranchId: null,
  publicationPrNumber: 42,
  publicationPrUrl: "https://example.test/pr/42",
  publicationPrStatus: "open",
  publicationPushStatus: "refreshed",
  maintenanceOperation: {
    operationId: "repair-1",
    generation: 2,
    source: "pr_autofix",
    stage: "held",
    status: "held",
    holdReason: "pr_autofix_unchanged_health",
    summary: "The fixer made no change.",
    blocker: null,
    automaticContinuation: false,
    startedAt: "2026-08-02T10:00:00Z",
    updatedAt: "2026-08-02T10:05:00Z",
  },
  prAutofixFingerprintSpend: {
    generations: 2,
    minutes: 18,
    budgetMinutes: 45,
    isExhausted: false,
  },
  status: "active",
  createdAt: "2026-08-02T10:00:00Z",
  updatedAt: "2026-08-02T10:05:00Z",
};

function workspaceWithHoldReason(
  holdReason: AgentWorkspaceMaintenanceOperationHoldReason,
): AgentConversationWorkspace {
  return {
    ...baseWorkspace,
    maintenanceOperation: {
      ...baseWorkspace.maintenanceOperation!,
      holdReason,
      // No durable summary or narrative here: these cases isolate the per-hold-reason
      // template paragraph itself, which the summary/narrative fallback tiers deliberately
      // outrank when either is present (see the dedicated narrative-override test below).
      summary: null,
    },
  };
}

function renderCard(
  workspace: AgentConversationWorkspace,
  overrides: Partial<{
    onRecheck: () => void;
    onRetry: () => void;
    onRetryPublication: () => void;
    onRerunChecks: () => void;
    onStop: () => void;
    isPending: boolean;
  }> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const handlers = {
    onRecheck: vi.fn(),
    onRetry: vi.fn(),
    onRetryPublication: vi.fn(),
    onRerunChecks: vi.fn(),
    onStop: vi.fn(),
    ...overrides,
  };
  const result = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <AgentsPublishHoldCard
          workspace={workspace}
          onRecheck={handlers.onRecheck}
          onRetry={handlers.onRetry}
          onRetryPublication={handlers.onRetryPublication}
          onRerunChecks={handlers.onRerunChecks}
          onStop={handlers.onStop}
          isPending={overrides.isPending}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return { ...result, ...handlers };
}

describe("AgentsPublishHoldCard", () => {
  it("renders the four-layer strip and routes the primary/secondary actions", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    const user = userEvent.setup();
    const { onRecheck, onRetry } = renderCard(baseWorkspace);

    const card = screen.getByTestId("agents-publish-hold-card");
    expect(within(card).getByText("Auto-repair paused")).toBeVisible();
    expect(within(card).getByText("Waiting on you")).toBeVisible();
    expect(
      within(card).getByText("2 generations · 18 min on this failure"),
    ).toBeVisible();
    expect(
      screen.getByText("Repair paused — waiting for new CI evidence"),
    ).toBeVisible();
    expect(
      screen.queryByTestId("agents-publish-change-facts"),
    ).not.toBeInTheDocument();

    const primary = screen.getByTestId("agents-publish-hold-primary");
    expect(primary).toHaveTextContent("Retry repair anyway");
    expect(
      screen.getByTestId("agents-publish-hold-primary-caption"),
    ).toHaveTextContent(/Spends another generation/);

    const secondary = screen.getByRole("button", { name: "Re-check PR health" });
    expect(secondary).toBeVisible();

    await user.click(primary);
    expect(onRetry).toHaveBeenCalledOnce();
    await user.click(secondary);
    expect(onRecheck).toHaveBeenCalledOnce();
  });

  it("keeps the primary caption visible without any hover interaction", () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    renderCard(baseWorkspace);

    const caption = screen.getByTestId("agents-publish-hold-primary-caption");
    expect(caption).toBeVisible();
    expect(caption.className).not.toMatch(/opacity-0/);
    expect(caption.className).not.toMatch(/group-hover/);
  });

  it("puts Stop auto-repair in the More menu with its scope caption", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    const user = userEvent.setup();
    const { onStop } = renderCard(baseWorkspace);

    expect(
      screen.queryByRole("button", { name: "Stop auto-repair" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-publish-hold-more-trigger"));
    const stopItem = await screen.findByTestId("agents-publish-hold-more-stop");
    expect(stopItem).toHaveTextContent("Stop auto-repair");
    expect(stopItem).toHaveTextContent(
      "Stops RalphX from retrying this failure automatically.",
    );

    await user.click(stopItem);
    expect(onStop).toHaveBeenCalledOnce();
  });

  it.each<[AgentWorkspaceMaintenanceOperationHoldReason, string, string]>([
    [
      "pr_autofix_unchanged_health",
      "Repair paused — waiting for new CI evidence",
      "The fixer ran but changed nothing, and GitHub still reports the same failing check. RalphX won't spend another generation until this PR's health changes.",
    ],
    [
      "pr_autofix_pre_existing_on_base",
      "Repair paused — failure exists on the base branch",
      "This failure already exists on the base branch, so fixing it on this PR branch would not help.",
    ],
    [
      "pr_autofix_ci_rerun_pending",
      "Repair paused — waiting for CI rerun",
      "RalphX asked GitHub to re-run the failed jobs and is waiting for the result.",
    ],
    [
      "base_stale",
      "Behind base — update did not take",
      "The branch is still behind its targeted base commit after RalphX attempted the update.",
    ],
    [
      "health_evidence",
      "Holding — waiting for new CI evidence",
      "RalphX is waiting for GitHub to report new PR health evidence before retrying.",
    ],
    [
      "publish_redrive",
      "Pushing rebased branch…",
      "RalphX is resuming publication for the rebased branch.",
    ],
    [
      "publication_effect_attention",
      "Repair paused — publish not confirmed",
      "RalphX can't confirm whether an earlier publish step reached GitHub. It stopped rather than risk pushing twice. Retry publication to have it check again and continue.",
    ],
    [
      "pr_autofix_base_parity_transient",
      "Repair paused — checks were cancelled or timed out",
      "GitHub cancelled or timed out the checks, and the same failure is present on the base branch. Re-running the checks can clear this.",
    ],
  ])(
    "renders the pill, court chip, headline, and paragraph for %s",
    (holdReason, headline, paragraph) => {
      listAgentConversationWorkspacePublicationEventsMock.mockReset();
      renderCard(workspaceWithHoldReason(holdReason));

      expect(screen.getByTestId("agents-publish-hold-pill")).toHaveTextContent(
        "Auto-repair paused",
      );
      expect(
        screen.getByTestId("agents-publish-hold-court-chip"),
      ).toBeInTheDocument();
      expect(screen.getByText(headline)).toBeVisible();
      expect(screen.getByText(paragraph)).toBeVisible();
    },
  );

  it("renders the agent's own narrative instead of the template paragraph", () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    const held = workspaceWithHoldReason("pr_autofix_base_parity_transient");
    renderCard({
      ...held,
      maintenanceOperation: {
        ...held.maintenanceOperation!,
        whatHappened: "GitHub cancelled the test job before it started.",
        whatIDid: "Left the branch untouched so a re-run can pick it up.",
      },
    });

    expect(
      screen.getByText(
        "GitHub cancelled the test job before it started. Left the branch untouched so a re-run can pick it up.",
      ),
    ).toBeVisible();
    expect(
      screen.queryByText(
        "GitHub cancelled or timed out the checks, and the same failure is present on the base branch. Re-running the checks can clear this.",
      ),
    ).not.toBeInTheDocument();
  });

  it("gates a publication_effect_attention hold to a single Retry publication action", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    const user = userEvent.setup();
    const { onRetryPublication } = renderCard(
      workspaceWithHoldReason("publication_effect_attention"),
    );

    const retryPublicationButton = screen.getByRole("button", {
      name: "Retry publication",
    });
    expect(retryPublicationButton).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Re-check PR health" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Retry repair anyway/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-publish-hold-more-trigger"),
    ).not.toBeInTheDocument();

    await user.click(retryPublicationButton);
    expect(onRetryPublication).toHaveBeenCalledOnce();
  });

  it("wires the rerun-checks primary action for a base-parity-transient hold", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    const user = userEvent.setup();
    const { onRerunChecks, onRetry } = renderCard(
      workspaceWithHoldReason("pr_autofix_base_parity_transient"),
    );

    const primary = screen.getByRole("button", { name: "Re-run failed checks" });
    const secondary = screen.getByRole("button", { name: /Retry repair anyway/i });
    await user.click(primary);
    expect(onRerunChecks).toHaveBeenCalledOnce();
    await user.click(secondary);
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("does not fetch the drawer timeline before Details is expanded", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    renderCard(baseWorkspace);

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(listAgentConversationWorkspacePublicationEventsMock).not.toHaveBeenCalled();
  });

  it("lazily fetches the drawer timeline only on first expand", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([
      {
        id: "event-1",
        conversationId: "conversation-1",
        step: "repair_held",
        status: "blocked",
        summary: "RalphX paused this repair.",
        classification: null,
        attemptId: "repair-1",
        createdAt: "2026-08-02T10:05:00Z",
      } satisfies AgentConversationWorkspacePublicationEvent,
    ]);
    const user = userEvent.setup();
    renderCard(baseWorkspace);

    expect(listAgentConversationWorkspacePublicationEventsMock).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("agents-publish-hold-details-trigger"));

    await waitFor(() =>
      expect(listAgentConversationWorkspacePublicationEventsMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    await waitFor(() =>
      expect(
        screen.getByText("RalphX paused this repair."),
      ).toBeVisible(),
    );
    expect(listAgentConversationWorkspacePublicationEventsMock).toHaveBeenCalledOnce();

    await user.click(screen.getByTestId("agents-publish-hold-details-trigger"));
    await user.click(screen.getByTestId("agents-publish-hold-details-trigger"));
    expect(listAgentConversationWorkspacePublicationEventsMock).toHaveBeenCalledOnce();
  });

  it("does not render the Technical details block when technicalDetails is null", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    const user = userEvent.setup();
    renderCard(workspaceWithHoldReason("publication_effect_attention"));

    expect(
      screen.queryByTestId("agents-publish-hold-technical-details"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-publish-hold-details-trigger"));

    expect(
      screen.queryByTestId("agents-publish-hold-technical-details"),
    ).not.toBeInTheDocument();
  });

  it("does not render the Technical details block when collapsed", () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    const workspaceWithTechDetails: AgentConversationWorkspace = {
      ...workspaceWithHoldReason("publication_effect_attention"),
      maintenanceOperation: {
        ...baseWorkspace.maintenanceOperation!,
        holdReason: "publication_effect_attention",
        summary: "RalphX retained the effect fence and did not reacquire or release Git authority.",
      },
    };
    renderCard(workspaceWithTechDetails);

    expect(
      screen.queryByTestId("agents-publish-hold-technical-details"),
    ).not.toBeInTheDocument();
  });

  it("shows Technical details inside the Details disclosure once expanded", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    const user = userEvent.setup();
    const workspaceWithTechDetails: AgentConversationWorkspace = {
      ...workspaceWithHoldReason("publication_effect_attention"),
      maintenanceOperation: {
        ...baseWorkspace.maintenanceOperation!,
        holdReason: "publication_effect_attention",
        summary: "RalphX retained the effect fence and did not reacquire or release Git authority.",
      },
    };
    renderCard(workspaceWithTechDetails);

    expect(
      screen.queryByTestId("agents-publish-hold-technical-details"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-publish-hold-details-trigger"));

    const techDetails = await screen.findByTestId("agents-publish-hold-technical-details");
    expect(techDetails).toBeVisible();
    expect(techDetails).toHaveTextContent(
      "RalphX retained the effect fence and did not reacquire or release Git authority.",
    );
    expect(techDetails).toHaveTextContent("Technical details");
  });

  it("renders release conditions and the auto-repair explainer once expanded", async () => {
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    const user = userEvent.setup();
    renderCard(baseWorkspace);

    await user.click(screen.getByTestId("agents-publish-hold-details-trigger"));

    expect(
      screen.getByText(
        "GitHub reports a different failing check or a passing result for this PR.",
      ),
    ).toBeVisible();
    expect(screen.getByTestId("agents-publish-hold-explainer")).toHaveTextContent(
      "What is auto-repair?",
    );
  });
});

describe("selectHoldDrawerTimeline", () => {
  it("keeps a blocked-status hold event instead of filtering it out like selectPublishHistory", () => {
    const blockedEvent: AgentConversationWorkspacePublicationEvent = {
      id: "event-blocked",
      conversationId: "conversation-1",
      step: "repair_held",
      status: "blocked",
      summary: "RalphX paused this repair.",
      classification: null,
      attemptId: "repair-1",
      createdAt: "2026-08-02T10:05:00Z",
    };

    const result = selectHoldDrawerTimeline([blockedEvent]);

    expect(result).toEqual([blockedEvent]);
  });

  it("dedupes by id and bounds the returned timeline, newest first", () => {
    const events: AgentConversationWorkspacePublicationEvent[] = Array.from(
      { length: 25 },
      (_, index) => ({
        id: `event-${index}`,
        conversationId: "conversation-1",
        step: "repair_step",
        status: "blocked",
        summary: `Step ${index}`,
        classification: null,
        attemptId: "repair-1",
        createdAt: `2026-08-02T10:${String(index).padStart(2, "0")}:00Z`,
      }),
    );
    const withDuplicate = [...events, events[0]!];

    const result = selectHoldDrawerTimeline(withDuplicate);

    expect(result).toHaveLength(20);
    expect(result[0]!.id).toBe("event-24");
    expect(result.at(-1)!.id).toBe("event-5");
  });
});
