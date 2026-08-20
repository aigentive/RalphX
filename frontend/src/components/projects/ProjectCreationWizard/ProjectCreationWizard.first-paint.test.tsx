/**
 * Focused test for .claude/rules/frontend-interaction-performance.md:
 * an intent click must paint the next step synchronously, and the
 * inspect_project_candidate probe must fire only after a debounced paint
 * boundary, never in the same synchronous commit as folder selection.
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { ProjectCreationWizard } from "./ProjectCreationWizard";

describe("ProjectCreationWizard first paint vs deferred IPC", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the Existing Repository step synchronously on intent click, with no probe call", () => {
    const onInspectCandidate = vi.fn().mockResolvedValue({ kind: "emptyDirectory" });

    render(
      <ProjectCreationWizard
        isOpen
        onClose={vi.fn()}
        onCreate={vi.fn()}
        onInspectCandidate={onInspectCandidate}
      />
    );

    fireEvent.click(screen.getByTestId("intent-existing"));

    // The step swap is a synchronous state update - no act()/await needed to observe it.
    expect(screen.getByTestId("folder-input")).toBeInTheDocument();
    expect(onInspectCandidate).not.toHaveBeenCalled();
  });

  it("defers inspect_project_candidate past a paint boundary after folder selection", async () => {
    vi.useFakeTimers();
    const onInspectCandidate = vi.fn().mockResolvedValue({ kind: "emptyDirectory" });
    const onBrowseFolder = vi.fn().mockResolvedValue("/Users/dev/my-app");

    render(
      <ProjectCreationWizard
        isOpen
        onClose={vi.fn()}
        onCreate={vi.fn()}
        onBrowseFolder={onBrowseFolder}
        onInspectCandidate={onInspectCandidate}
      />
    );

    fireEvent.click(screen.getByTestId("intent-existing"));

    await act(async () => {
      fireEvent.click(screen.getByTestId("browse-button"));
      await Promise.resolve();
      await Promise.resolve();
    });

    // Folder path is committed to the DOM before the debounced probe fires.
    expect(screen.getByTestId("folder-input")).toHaveValue("/Users/dev/my-app");
    expect(onInspectCandidate).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });

    expect(onInspectCandidate).toHaveBeenCalledWith("/Users/dev/my-app");
  });
});
