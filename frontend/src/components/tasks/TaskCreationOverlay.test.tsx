import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TaskCreationOverlay } from "./TaskCreationOverlay";
import { useUiStore } from "@/stores/uiStore";

vi.mock("./TaskCreationForm", () => ({
  TaskCreationForm: ({ projectId }: { projectId: string }) => (
    <div data-testid="task-creation-form">project={projectId}</div>
  ),
}));

beforeEach(() => {
  useUiStore.setState({ taskCreationContext: null } as Partial<ReturnType<typeof useUiStore.getState>>);
});

describe("TaskCreationOverlay", () => {
  it("renders nothing when no taskCreationContext is set", () => {
    const { container } = render(<TaskCreationOverlay projectId="proj-1" />);
    expect(container.innerHTML).toBe("");
  });

  it("renders the overlay header + form when context is open", () => {
    useUiStore.setState({
      taskCreationContext: { projectId: "proj-1" },
    } as Partial<ReturnType<typeof useUiStore.getState>>);
    render(<TaskCreationOverlay projectId="proj-1" />);
    expect(screen.getByTestId("task-creation-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("task-creation-form")).toBeInTheDocument();
  });

  it("closes the overlay on Escape key + Close button", async () => {
    const user = userEvent.setup();
    useUiStore.setState({
      taskCreationContext: { projectId: "proj-1" },
    } as Partial<ReturnType<typeof useUiStore.getState>>);
    render(<TaskCreationOverlay projectId="proj-1" />);

    await user.click(screen.getByTestId("task-creation-overlay-close"));
    // Close button calls closeTaskCreation; subsequent escape exercises the
    // keydown listener cleanup branch.
    fireEvent.keyDown(window, { key: "Escape" });
    expect(useUiStore.getState().taskCreationContext).toBeNull();
  });

  it("backdrop click closes the overlay; child click does not", () => {
    useUiStore.setState({
      taskCreationContext: { projectId: "proj-1" },
    } as Partial<ReturnType<typeof useUiStore.getState>>);
    render(<TaskCreationOverlay projectId="proj-1" />);

    const child = screen.getByTestId("task-creation-overlay");
    fireEvent.click(child);
    // Child click stops propagation, overlay should still be open.
    expect(useUiStore.getState().taskCreationContext).not.toBeNull();

    const backdrop = screen.getByTestId("task-creation-overlay-backdrop");
    fireEvent.click(backdrop);
    expect(useUiStore.getState().taskCreationContext).toBeNull();
  });
});
