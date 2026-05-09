import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import WelcomeScreen from "./WelcomeScreen";

vi.mock("./AgentConstellation", () => ({
  default: () => <div data-testid="agent-constellation" />,
}));

describe("WelcomeScreen", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("routes provider onboarding actions to provider setup", async () => {
    const user = userEvent.setup();
    const onCreateProject = vi.fn();
    const onSetupProviders = vi.fn();

    render(
      <WelcomeScreen
        onCreateProject={onCreateProject}
        onSetupProviders={onSetupProviders}
        providerSetupRequired
        hasProjects
      />,
    );

    expect(
      screen.getByText("The best way to ship software with AI"),
    ).toBeInTheDocument();
    expect(screen.getByText("Choose your agent harness.")).toBeInTheDocument();
    expect(screen.getByText("Project workspace ready.")).toBeInTheDocument();
    expect(screen.getByTestId("welcome-setup-steps")).toBeInTheDocument();
    expect(screen.getByTestId("welcome-project-step")).toHaveAttribute(
      "data-status",
      "complete",
    );

    await user.click(screen.getByRole("button", { name: /Set Up Provider/ }));
    fireEvent.keyDown(window, { key: "n", metaKey: true });

    expect(onSetupProviders).toHaveBeenCalledTimes(2);
    expect(onCreateProject).not.toHaveBeenCalled();
  });

  it("shows the project setup subtitle when provider setup is required before a first project", () => {
    render(
      <WelcomeScreen
        onCreateProject={vi.fn()}
        onSetupProviders={vi.fn()}
        providerSetupRequired
        hasProjects={false}
      />,
    );

    expect(screen.getByText("Choose your agent harness.")).toBeInTheDocument();
    expect(screen.getByText("Create your first project.")).toBeInTheDocument();
    expect(screen.getByTestId("welcome-project-step")).toHaveAttribute(
      "data-status",
      "pending",
    );
  });

  it("keeps the first-project action when provider setup is not required", () => {
    const onCreateProject = vi.fn();

    render(<WelcomeScreen onCreateProject={onCreateProject} />);

    expect(
      screen.getByText("The best way to ship software with AI"),
    ).toBeInTheDocument();
    expect(screen.getByText("Agent harness ready.")).toBeInTheDocument();
    expect(screen.getByText("Create your first project.")).toBeInTheDocument();
    expect(screen.getByTestId("welcome-provider-step")).toHaveAttribute(
      "data-status",
      "complete",
    );
    expect(screen.getByTestId("welcome-project-step")).toHaveAttribute(
      "data-status",
      "current",
    );
    expect(
      screen.getByRole("button", { name: /Start Your First Project/ }),
    ).toBeInTheDocument();
    expect(screen.getByText(/to create a project/)).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "n", metaKey: true });

    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("shows completed onboarding state and closes from the manual welcome overlay", async () => {
    const user = userEvent.setup();
    const onCreateProject = vi.fn();
    const onClose = vi.fn();

    render(
      <WelcomeScreen
        onCreateProject={onCreateProject}
        hasProjects
        onClose={onClose}
      />,
    );

    expect(
      screen.getByText("The best way to ship software with AI"),
    ).toBeInTheDocument();
    expect(screen.getByText("Agent harness ready.")).toBeInTheDocument();
    expect(screen.getByText("Project workspace ready.")).toBeInTheDocument();
    expect(screen.getByTestId("welcome-provider-step")).toHaveAttribute(
      "data-status",
      "complete",
    );
    expect(screen.getByTestId("welcome-project-step")).toHaveAttribute(
      "data-status",
      "complete",
    );
    expect(screen.queryByText(/to create a project/)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Continue/ }));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onCreateProject).not.toHaveBeenCalled();
  });

  it("ignores create-project shortcuts while typing", () => {
    const onCreateProject = vi.fn();

    render(
      <>
        <input aria-label="Project name" />
        <WelcomeScreen onCreateProject={onCreateProject} />
      </>,
    );

    screen.getByLabelText("Project name").focus();
    fireEvent.keyDown(window, { key: "n", metaKey: true });

    expect(onCreateProject).not.toHaveBeenCalled();
  });
});
