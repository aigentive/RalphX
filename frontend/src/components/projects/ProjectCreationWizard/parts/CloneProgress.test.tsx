/**
 * Tests for CloneProgress (presentational).
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CloneProgress } from "./CloneProgress";

describe("CloneProgress", () => {
  it("renders a determinate bar with the percent when known", () => {
    render(
      <CloneProgress
        phase="receiving"
        percent={42}
        received={420}
        total={1000}
        lines={[]}
        onCancel={vi.fn()}
      />
    );

    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("aria-valuenow", "42");
    expect(screen.getByText("42%")).toBeInTheDocument();
    expect(screen.getByText("420 / 1,000 objects")).toBeInTheDocument();
  });

  it("renders an indeterminate bar with a note during checking_out", () => {
    render(
      <CloneProgress
        phase="checking_out"
        percent={null}
        received={null}
        total={null}
        lines={[]}
        onCancel={vi.fn()}
      />
    );

    const bar = screen.getByRole("progressbar");
    expect(bar).not.toHaveAttribute("aria-valuenow");
    expect(screen.getByText(/this can take a while for large repositories/i)).toBeInTheDocument();
  });

  it("does not show the large-repository note outside checking_out", () => {
    render(
      <CloneProgress phase="connecting" percent={null} received={null} total={null} lines={[]} onCancel={vi.fn()} />
    );

    expect(screen.queryByText(/this can take a while/i)).not.toBeInTheDocument();
  });

  it("expands the collapsible console to show raw lines", async () => {
    const user = userEvent.setup();
    render(
      <CloneProgress
        phase="receiving"
        percent={10}
        received={null}
        total={null}
        lines={["remote: Counting objects: 10", "Receiving objects: 10% (1/10)"]}
        onCancel={vi.fn()}
      />
    );

    await user.click(screen.getByTestId("clone-console-trigger"));
    const output = screen.getByTestId("clone-console-output");
    expect(output).toHaveTextContent("remote: Counting objects: 10");
    expect(output).toHaveTextContent("Receiving objects: 10% (1/10)");
  });

  it("calls onCancel when Cancel Clone is clicked, and disables it while cancelling", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const { rerender } = render(
      <CloneProgress phase="receiving" percent={10} received={null} total={null} lines={[]} onCancel={onCancel} />
    );

    await user.click(screen.getByTestId("clone-cancel-button"));
    expect(onCancel).toHaveBeenCalledTimes(1);

    rerender(
      <CloneProgress
        phase="receiving"
        percent={10}
        received={null}
        total={null}
        lines={[]}
        onCancel={onCancel}
        isCancelling
      />
    );
    expect(screen.getByTestId("clone-cancel-button")).toBeDisabled();
  });
});
