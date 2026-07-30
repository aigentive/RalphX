import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import type { IdeationSettingsController } from "../IdeationSettingsPanel";
import CapacitySettingsSection from "./CapacitySettingsSection";
import PlanningSettingsSection from "./PlanningSettingsSection";
import TasksSettingsSection from "./TasksSettingsSection";
import WorkspaceSettingsSection from "./WorkspaceSettingsSection";

vi.mock("../IdeationSettingsPanel", () => ({
  IdeationSettingsContent: ({ surface }: { surface: string }) => (
    <div>Ideation surface: {surface}</div>
  ),
}));

vi.mock("./ReviewPolicySection", () => ({
  default: () => <div>Review policy content</div>,
}));

vi.mock("./AutonomyPolicySection", () => ({
  default: () => <div>Autonomy policy content</div>,
}));

vi.mock("./WorkspaceReviewSection", () => ({
  default: () => <div>Workspace review content</div>,
}));

vi.mock("./ExecutionSection", () => ({
  default: ({ content }: { content: string }) => <div>Execution content: {content}</div>,
}));

vi.mock("./GlobalExecutionSection", () => ({
  default: () => <div>Global capacity content</div>,
}));

const controller = {} as IdeationSettingsController;

describe("settings composite sections", () => {
  it("routes Tasks deep links to the requested tab and supports tab changes", async () => {
    const user = userEvent.setup();
    render(<TasksSettingsSection controller={controller} initialTab="review-policy" />);

    expect(screen.getByText("Review policy content")).toBeInTheDocument();
    expect(screen.queryByText("Ideation surface: tasks")).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Autonomy Policy" }));
    expect(screen.getByText("Autonomy policy content")).toBeInTheDocument();
  });

  it("renders Planning from the same ideation controller on its isolated surface", () => {
    render(<PlanningSettingsSection controller={controller} />);

    expect(screen.getByText("Plan verification")).toBeInTheDocument();
    expect(screen.getByText("Ideation surface: planning")).toBeInTheDocument();
  });

  it("routes Workspace deep links between General and Review", async () => {
    const { rerender } = render(
      <WorkspaceSettingsSection
        settings={DEFAULT_PROJECT_SETTINGS}
        disabled={false}
        onSettingsChange={vi.fn()}
        initialTab="review"
      />
    );

    expect(screen.getByText("Workspace review content")).toBeInTheDocument();

    rerender(
      <WorkspaceSettingsSection
        settings={DEFAULT_PROJECT_SETTINGS}
        disabled={false}
        onSettingsChange={vi.fn()}
        initialTab="general"
      />
    );

    await waitFor(() => {
      expect(screen.getByText("Execution content: workspace")).toBeInTheDocument();
    });
  });

  it("keeps project and global concurrency together under Capacity", () => {
    render(
      <CapacitySettingsSection
        settings={DEFAULT_PROJECT_SETTINGS}
        disabled={false}
        onSettingsChange={vi.fn()}
      />
    );

    expect(screen.getByText("Execution content: capacity")).toBeInTheDocument();
    expect(screen.getByText("Global capacity content")).toBeInTheDocument();
  });
});
