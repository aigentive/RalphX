import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";

import { chatApi } from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import { createTestQueryClient } from "@/test/store-utils";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { AgentsPublishAutomationTab } from "./AgentsPublishAutomationTab";
import { hasReportableAutofixSpend } from "./agentWorkspacePublishState";
import {
  deriveAgentsPublishAutomationSnapshot,
  hasActiveAgentsPublishAutomation,
} from "./agentsPublishAutomationSnapshot";
import { WORKSPACE_REVIEW_AUTOMATION_COPY } from "./workspaceReviewAutomationCopy";

afterEach(() => {
  vi.restoreAllMocks();
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderAutomationTab(
  workspace = conversationWorkspaceFixture({
    mode: "edit",
    autoPublishInitialPrEnabled: false,
  }),
) {
  const onSnapshotChange = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <TooltipProvider delayDuration={0}>
        <AgentsPublishAutomationTab
          workspace={workspace}
          hasPublishedPr={workspace.publicationPrNumber !== null}
          isPipelinePrAutomationWorkspace={false}
          canConfigurePrSupervision
          hasUncommittedChanges={false}
          terminalPrLabel="This pull request"
          onSnapshotChange={onSnapshotChange}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return { onSnapshotChange };
}

describe("AgentsPublishAutomationTab", () => {
  it("applies pending and settled automation precedence in one derivation", () => {
    const workspace = conversationWorkspaceFixture({
      autoPublishEnabled: false,
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
      prAutoMergeCurrent: false,
      prSupervisionStatus: "disabled",
      publicationPrNumber: 78,
      reviewAutomationOverride: true,
    });
    const settledWorkspace = {
      ...workspace,
      prAutofixEnabled: true,
      prAutoMergeDesired: true,
      prAutoMergeCurrent: true,
      prSupervisionStatus: "monitoring" as const,
      reviewAutomationOverride: null,
    };

    expect(
      deriveAgentsPublishAutomationSnapshot({
        workspace,
        hasPublishedPr: true,
        pendingAutoPublish: { autoPublishEnabled: true },
        pendingPrSupervision: {
          autoFixEnabled: false,
          autoMergeDesired: false,
        },
        pendingReviewAutomation: { enabled: null },
        settledPrSupervisionWorkspace: settledWorkspace,
        settledReviewAutomationWorkspace: settledWorkspace,
        isAutoPublishSaving: true,
        isPrSupervisionSaving: true,
      }),
    ).toMatchObject({
      conversationId: workspace.conversationId,
      autoPublishEnabled: true,
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
      prAutoMergeCurrent: true,
      prSupervisionStatus: "monitoring",
      reviewAutomationOverride: null,
      isAutoPublishSaving: true,
      isPrSupervisionSaving: true,
    });

    expect(
      deriveAgentsPublishAutomationSnapshot({
        workspace,
        hasPublishedPr: true,
        settledReviewAutomationWorkspace: settledWorkspace,
      }).reviewAutomationOverride,
    ).toBeNull();
  });

  it("renders the existing controls as accessible WebKit-safe cards", async () => {
    renderAutomationTab();

    expect(
      screen.getByRole("switch", { name: "Auto Publish" }),
    ).toHaveAttribute("data-testid", "agents-auto-publish-switch");
    expect(
      screen.getByRole("switch", { name: "Autofix CI & Reviews" }),
    ).toHaveAttribute("data-testid", "agents-pr-autofix-switch");
    expect(
      screen.getByRole("switch", { name: "GitHub auto-merge" }),
    ).toHaveAttribute("data-testid", "agents-pr-auto-merge-switch");
    expect(
      screen.getByRole("button", { name: "About Auto Publish" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "About Auto Review & Fix" }),
    ).not.toBeInTheDocument();
    expect(screen.getAllByText(WORKSPACE_REVIEW_AUTOMATION_COPY)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Inherit" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "On" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Off" })).toBeInTheDocument();

    const cards = screen
      .getByTestId("agents-pr-supervision-controls")
      .querySelectorAll(":scope > div");
    expect(cards).toHaveLength(4);
    for (const card of cards) {
      expect(card.getAttribute("style")).toContain(
        "background-color: var(--bg-subtle)",
      );
      expect(card.getAttribute("style")).toContain(
        "border-color: var(--border-subtle)",
      );
      expect(card.getAttribute("style")).toContain("border-style: solid");
      expect(card.getAttribute("style")).toContain("border-width: 1px");
    }
  });

  it("confirms and snapshots the explicit Auto Review & Fix override", async () => {
    const user = userEvent.setup();
    const workspace = conversationWorkspaceFixture({
      reviewAutomationOverride: null,
    });
    const mutation = vi
      .spyOn(chatApi, "setAgentConversationWorkspaceReviewAutomation")
      .mockResolvedValue({ ...workspace, reviewAutomationOverride: true });
    const { onSnapshotChange } = renderAutomationTab(workspace);

    await user.click(screen.getByRole("button", { name: "On" }));
    await user.click(
      screen.getByRole("button", { name: "Turn on Auto Review & Fix" }),
    );

    await waitFor(() =>
      expect(mutation).toHaveBeenCalledWith(workspace.conversationId, {
        enabled: true,
      }),
    );
    await waitFor(() =>
      expect(onSnapshotChange).toHaveBeenLastCalledWith(
        expect.objectContaining({ reviewAutomationOverride: true }),
      ),
    );
  });

  it("returns Auto Review & Fix to the global preference", async () => {
    const user = userEvent.setup();
    const workspace = conversationWorkspaceFixture({
      reviewAutomationOverride: true,
    });
    const result = deferred<typeof workspace>();
    const mutation = vi
      .spyOn(chatApi, "setAgentConversationWorkspaceReviewAutomation")
      .mockReturnValue(result.promise);
    renderAutomationTab(workspace);

    await user.click(screen.getByRole("button", { name: "Inherit" }));
    await user.click(
      screen.getByRole("button", { name: "Use global Review settings" }),
    );

    await waitFor(() =>
      expect(mutation).toHaveBeenCalledWith(workspace.conversationId, {
        enabled: null,
      }),
    );
    expect(screen.getByText("Inherit", { selector: "button" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    result.resolve({ ...workspace, reviewAutomationOverride: null });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Inherit" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
  });

  it("marks automation active only for an explicit review override", () => {
    const base = deriveAgentsPublishAutomationSnapshot({
      workspace: conversationWorkspaceFixture({
        autoPublishEnabled: false,
        prAutofixEnabled: false,
        prAutoMergeDesired: false,
        reviewAutomationOverride: null,
      }),
      hasPublishedPr: true,
    });

    expect(hasActiveAgentsPublishAutomation(base)).toBe(false);
    expect(
      hasActiveAgentsPublishAutomation({
        ...base,
        reviewAutomationOverride: true,
      }),
    ).toBe(true);
    expect(
      hasActiveAgentsPublishAutomation({
        ...base,
        reviewAutomationOverride: false,
      }),
    ).toBe(false);
  });

  it("keeps the extracted coordinator as the PR preference mutation writer", async () => {
    const workspace = conversationWorkspaceFixture({
      mode: "edit",
      autoPublishEnabled: true,
      publicationPrNumber: 78,
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
    });
    const mutation = vi
      .spyOn(chatApi, "setAgentConversationWorkspacePrSupervision")
      .mockResolvedValue({
        ...workspace,
        prAutofixEnabled: true,
        prSupervisionStatus: "monitoring",
      });
    const { onSnapshotChange } = renderAutomationTab(workspace);

    fireEvent.click(
      screen.getByRole("switch", { name: "Autofix CI & Reviews" }),
    );

    await waitFor(() =>
      expect(mutation).toHaveBeenCalledWith(workspace.conversationId, {
        autoFixEnabled: true,
        autoMergeDesired: false,
        autoMergeMethod: workspace.prAutoMergeMethod ?? "squash",
      }),
    );
    await waitFor(() =>
      expect(onSnapshotChange).toHaveBeenLastCalledWith(
        expect.objectContaining({
          conversationId: workspace.conversationId,
          prAutofixEnabled: true,
        }),
      ),
    );
  });

  it("shows the current repair budget spend line and never renders a raw event ledger", () => {
    const workspace = conversationWorkspaceFixture({
      prAutofixFingerprintSpend: {
        generations: 2,
        minutes: 18,
        budgetMinutes: 45,
        isExhausted: false,
      },
    });
    renderAutomationTab(workspace);

    expect(screen.getByTestId("agents-pr-autofix-budget")).toHaveTextContent(
      "18 / 45 min",
    );
    expect(screen.getByTestId("agents-pr-autofix-budget")).toHaveTextContent(
      "2 generations",
    );
    expect(
      screen.queryByTestId("agents-pr-autofix-ledger"),
    ).not.toBeInTheDocument();
  });

  it("hides the budget card when spend is null", () => {
    const workspace = conversationWorkspaceFixture({
      prAutofixFingerprintSpend: null,
    });
    renderAutomationTab(workspace);

    expect(
      screen.queryByTestId("agents-pr-autofix-budget"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-pr-autofix-ledger"),
    ).not.toBeInTheDocument();
  });

  it("hides the budget card when spend is present but zero and not exhausted, even though repair publication events exist for this backend state", () => {
    // Backend returns Some({ generations: 0, minutes: 0, isExhausted: false }) for any
    // workspace with a pr_autofix_fingerprint, even one that never spent anything and has
    // repair-related publication events on record. A null gate would render this permanently.
    const workspace = conversationWorkspaceFixture({
      prAutofixFingerprintSpend: {
        generations: 0,
        minutes: 0,
        budgetMinutes: 45,
        isExhausted: false,
      },
    });
    renderAutomationTab(workspace);

    expect(
      screen.queryByTestId("agents-pr-autofix-budget"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-pr-autofix-ledger"),
    ).not.toBeInTheDocument();
  });

  it("shows the spend line once generations are non-zero", () => {
    const workspace = conversationWorkspaceFixture({
      prAutofixFingerprintSpend: {
        generations: 1,
        minutes: 0,
        budgetMinutes: 45,
        isExhausted: false,
      },
    });
    renderAutomationTab(workspace);

    expect(screen.getByTestId("agents-pr-autofix-budget")).toHaveTextContent(
      "0 / 45 min",
    );
  });

  it("shows the spend line once minutes are non-zero", () => {
    const workspace = conversationWorkspaceFixture({
      prAutofixFingerprintSpend: {
        generations: 0,
        minutes: 5,
        budgetMinutes: 45,
        isExhausted: false,
      },
    });
    renderAutomationTab(workspace);

    expect(screen.getByTestId("agents-pr-autofix-budget")).toHaveTextContent(
      "5 / 45 min",
    );
  });

  it("shows the exhausted treatment even at zero minutes and generations", () => {
    const workspace = conversationWorkspaceFixture({
      prAutofixFingerprintSpend: {
        generations: 0,
        minutes: 0,
        budgetMinutes: 45,
        isExhausted: true,
      },
    });
    renderAutomationTab(workspace);

    expect(screen.getByTestId("agents-pr-autofix-budget")).toHaveTextContent(
      "Repair budget exhausted",
    );
  });

  describe("hasReportableAutofixSpend", () => {
    it("is false for null spend", () => {
      expect(hasReportableAutofixSpend(null)).toBe(false);
    });

    it("is false for zeroed, non-exhausted spend", () => {
      expect(
        hasReportableAutofixSpend({
          generations: 0,
          minutes: 0,
          budgetMinutes: 45,
          isExhausted: false,
        }),
      ).toBe(false);
    });

    it("is true when generations, minutes, or exhaustion are reportable", () => {
      expect(
        hasReportableAutofixSpend({
          generations: 1,
          minutes: 0,
          budgetMinutes: 45,
          isExhausted: false,
        }),
      ).toBe(true);
      expect(
        hasReportableAutofixSpend({
          generations: 0,
          minutes: 1,
          budgetMinutes: 45,
          isExhausted: false,
        }),
      ).toBe(true);
      expect(
        hasReportableAutofixSpend({
          generations: 0,
          minutes: 0,
          budgetMinutes: 45,
          isExhausted: true,
        }),
      ).toBe(true);
    });
  });
});
