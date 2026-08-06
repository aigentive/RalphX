import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import type { AgentConversationWorkspace } from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AgentsPublishHoldCard } from "./AgentsPublishHoldCard";

const workspace: AgentConversationWorkspace = {
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

describe("AgentsPublishHoldCard", () => {
  it("renders durable hold facts and routes its real actions", async () => {
    const user = userEvent.setup();
    const onOpenChecks = vi.fn();
    const onRecheck = vi.fn();
    const onRetry = vi.fn();
    const onRetryPublication = vi.fn();
    const onStop = vi.fn();
    render(
      <TooltipProvider delayDuration={0}>
        <AgentsPublishHoldCard
          workspace={workspace}
          onOpenChecks={onOpenChecks}
          onRecheck={onRecheck}
          onRetry={onRetry}
          onRetryPublication={onRetryPublication}
          onStop={onStop}
        />
      </TooltipProvider>,
    );

    expect(screen.getByText("Repair paused — waiting for new CI evidence")).toBeVisible();
    expect(screen.getByText("Nothing is running")).toBeVisible();
    expect(screen.getByText("2 generations · 18 min on this failure")).toBeVisible();

    await user.click(screen.getByTestId("agents-publish-hold-open-checks"));
    await user.click(screen.getByRole("button", { name: "Re-check PR health" }));
    await user.click(screen.getByRole("button", { name: /Retry repair anyway/i }));
    await user.click(screen.getByRole("button", { name: "Stop auto-repair" }));

    expect(onOpenChecks).toHaveBeenCalledOnce();
    expect(onRecheck).toHaveBeenCalledOnce();
    expect(onRetry).toHaveBeenCalledOnce();
    expect(onStop).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", { name: "Retry publication" }),
    ).not.toBeInTheDocument();
    expect(onRetryPublication).not.toHaveBeenCalled();
  });

  it("gates a publication_effect_attention hold to a single Retry publication action", async () => {
    const user = userEvent.setup();
    const onOpenChecks = vi.fn();
    const onRecheck = vi.fn();
    const onRetry = vi.fn();
    const onRetryPublication = vi.fn();
    const onStop = vi.fn();
    const publicationEffectWorkspace: AgentConversationWorkspace = {
      ...workspace,
      maintenanceOperation: {
        ...workspace.maintenanceOperation!,
        source: "publish",
        holdReason: "publication_effect_attention",
        summary: "RalphX could not confirm the push reached GitHub.",
      },
    };
    render(
      <TooltipProvider delayDuration={0}>
        <AgentsPublishHoldCard
          workspace={publicationEffectWorkspace}
          onOpenChecks={onOpenChecks}
          onRecheck={onRecheck}
          onRetry={onRetry}
          onRetryPublication={onRetryPublication}
          onStop={onStop}
        />
      </TooltipProvider>,
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
      screen.queryByRole("button", { name: "Stop auto-repair" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Abandon/i }),
    ).not.toBeInTheDocument();

    await user.click(retryPublicationButton);
    expect(onRetryPublication).toHaveBeenCalledOnce();
    expect(onRecheck).not.toHaveBeenCalled();
    expect(onRetry).not.toHaveBeenCalled();
    expect(onStop).not.toHaveBeenCalled();
  });
});
