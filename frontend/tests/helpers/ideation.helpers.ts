import { Page } from "@playwright/test";

/**
 * Helper to select the mock ideation session that has proposals.
 * The mock data in frontend/src/api-mock/ideation.ts creates a session with one proposal.
 * This helper waits for that data to load and waits for the proposal cards.
 */
export async function loadMockIdeationSession(page: Page) {
  await page.evaluate(() => {
    window.__ideationStore?.getState().selectSession({
      id: "session-mock-1",
      projectId: "project-mock-1",
      title: "Demo Ideation Session",
      titleSource: null,
      status: "active",
      planArtifactId: null,
      seedTaskId: null,
      sourceTaskId: null,
      sourceContextType: null,
      sourceContextId: null,
      parentSessionId: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      archivedAt: null,
      convertedAt: null,
      verificationStatus: "unverified",
      verificationInProgress: false,
      gapScore: null,
      verificationUpdateSeq: 0,
      sessionPurpose: "general",
      acceptanceStatus: null,
    });

    window.__proposalStore?.getState().setProposals([
      {
        id: "proposal-mock-1",
        sessionId: "session-mock-1",
        title: "Sample Proposal",
        description: "A sample proposal for testing",
        category: "feature",
        steps: ["Step 1", "Step 2", "Step 3"],
        acceptanceCriteria: ["Criteria 1", "Criteria 2"],
        suggestedPriority: "medium",
        priorityScore: 50,
        priorityReason: "Medium complexity feature",
        estimatedComplexity: "medium",
        userPriority: null,
        userModified: false,
        status: "pending",
        createdTaskId: null,
        planArtifactId: null,
        planVersionAtCreation: null,
        sortOrder: 0,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
    ]);
  });

  // Wait for the session header to appear
  await page.waitForSelector('[data-testid="ideation-header"]', { timeout: 10000 });

  // Proposal cards now render behind the explicit Proposals tab instead of
  // always being present in the default Plan view.
  await page.click('[data-testid="tab-proposals"]');

  // Wait for proposal cards to appear
  await page.waitForSelector('[data-testid^="proposal-card-"]', { timeout: 10000 });
}

/**
 * Helper to open the ProposalEditModal for the first proposal in the loaded session.
 * Must be called after loadMockIdeationSession.
 */
export async function openProposalEditModal(page: Page) {
  const firstProposalCard = page.locator('[data-testid^="proposal-card-"]').first();

  // Hover to reveal edit button
  await firstProposalCard.hover();

  // Wait a moment for hover state to activate
  await page.waitForTimeout(200);

  // Click the first action button in the hovered card (edit icon).
  const editButton = firstProposalCard.locator("button").first();
  await editButton.click();

  // Wait for modal to appear
  await page.waitForSelector('[data-testid="proposal-edit-modal"]', { timeout: 5000 });
}
