import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";

import { chatApi } from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import { createTestQueryClient } from "@/test/store-utils";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { AgentsPublishAutomationTab } from "./AgentsPublishAutomationTab";
import {
  deriveAgentsPublishAutomationSnapshot,
  hasActiveAgentsPublishAutomation,
} from "./agentsPublishAutomationSnapshot";

afterEach(() => {
  vi.restoreAllMocks();
});

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
    });
    const settledWorkspace = {
      ...workspace,
      prAutofixEnabled: true,
      prAutoMergeDesired: true,
      prAutoMergeCurrent: true,
      prSupervisionStatus: "monitoring" as const,
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
        settledPrSupervisionWorkspace: settledWorkspace,
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
      isAutoPublishSaving: true,
      isPrSupervisionSaving: true,
    });
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
    const mutation = vi
      .spyOn(chatApi, "setAgentConversationWorkspaceReviewAutomation")
      .mockResolvedValue({ ...workspace, reviewAutomationOverride: null });
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
      hasActiveAgentsPublishAutomation({ ...base, reviewAutomationOverride: true }),
    ).toBe(true);
    expect(
      hasActiveAgentsPublishAutomation({ ...base, reviewAutomationOverride: false }),
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
});
