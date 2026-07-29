import { expect, type Locator, type Page } from "@playwright/test";

import { BasePage } from "../base.page";

export class AutomationDetailPage extends BasePage {
  readonly navAutomations: Locator;
  readonly detailView: Locator;
  readonly overviewTab: Locator;
  readonly runsTab: Locator;
  readonly runsTabLiveDot: Locator;
  readonly runsTimeline: Locator;
  readonly goalCard: Locator;
  readonly phasesCard: Locator;
  readonly statCards: Locator;
  readonly executionCard: Locator;
  readonly specInputsCard: Locator;
  readonly planDialog: Locator;

  constructor(page: Page) {
    super(page);
    this.navAutomations = page.getByTestId("nav-automations");
    this.detailView = page.getByTestId("automation-detail-view");
    this.overviewTab = page.getByTestId("automation-tab-overview");
    this.runsTab = page.getByTestId("automation-tab-runs");
    this.runsTabLiveDot = page.getByTestId("automation-tab-runs-live-dot");
    this.runsTimeline = page.getByTestId("automation-runs-timeline");
    this.goalCard = page.getByTestId("automation-goal-card");
    this.phasesCard = page.getByTestId("automation-phases-card");
    this.statCards = page.getByTestId("automation-stat-cards");
    this.executionCard = page.getByTestId("automation-execution-card");
    this.specInputsCard = page.getByTestId("automation-spec-inputs-card");
    this.planDialog = page.getByTestId("automation-plan-dialog");
  }

  automationRow(automationId: string): Locator {
    return this.page.getByTestId(`automation-row-${automationId}`);
  }

  runCard(runId: string): Locator {
    return this.page.getByTestId(`automation-run-${runId}-card`);
  }

  runPlanIcon(runId: string): Locator {
    return this.page.getByTestId(`automation-run-${runId}-plan-icon`);
  }

  async openAutomationsView(): Promise<void> {
    await this.navAutomations.click();
    await expect(this.page.getByTestId("automations-view-shell")).toBeVisible();
  }

  async openDetail(automationId: string): Promise<void> {
    await this.automationRow(automationId).click();
    await expect(this.detailView).toBeVisible();
    await expect(this.goalCard).toBeVisible();
  }

  async openRunsTab(): Promise<void> {
    await this.runsTab.click();
    await expect(this.runsTimeline).toBeVisible();
  }
}
