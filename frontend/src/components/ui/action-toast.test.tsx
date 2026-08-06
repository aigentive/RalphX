import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ActionToast } from "./action-toast";

describe("ActionToast", () => {
  it("renders title and description", () => {
    render(<ActionToast title="New notification" description="Something happened" />);
    expect(screen.getByText("New notification")).toBeVisible();
    expect(screen.getByText("Something happened")).toBeVisible();
  });

  it("renders Dismiss plus N actions, right-aligned row, in order (Dismiss first)", () => {
    render(
      <ActionToast
        title="Title"
        onDismiss={() => {}}
        actions={[{ label: "Open", onClick: () => {} }, { label: "Snooze", onClick: () => {} }]}
      />,
    );
    const row = screen.getByText("Dismiss").closest("div");
    expect(row).toHaveClass("flex", "items-center", "justify-end", "gap-1");
    const buttons = row ? Array.from(row.querySelectorAll("button")) : [];
    expect(buttons.map((b) => b.textContent)).toEqual(["Dismiss", "Open", "Snooze"]);
  });

  it("disables an async action while pending and re-enables after resolve; other action stays enabled", async () => {
    let resolveAction: () => void = () => {};
    const pendingPromise = new Promise<void>((resolve) => {
      resolveAction = resolve;
    });
    const onAction = vi.fn(() => pendingPromise);
    const onOther = vi.fn();

    render(
      <ActionToast
        title="Title"
        actions={[
          { label: "Approve", onClick: onAction },
          { label: "Other", onClick: onOther },
        ]}
      />,
    );

    const approveButton = screen.getByRole("button", { name: "Approve" });
    const otherButton = screen.getByRole("button", { name: "Other" });

    fireEvent.click(approveButton);

    await waitFor(() => expect(screen.getByRole("button", { name: "Approve…" })).toBeDisabled());
    expect(otherButton).toBeEnabled();

    resolveAction();

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled(),
    );
  });

  it("omits the Dismiss button when onDismiss is absent", () => {
    render(<ActionToast title="Title" actions={[{ label: "Open", onClick: () => {} }]} />);
    expect(screen.queryByText("Dismiss")).not.toBeInTheDocument();
  });

  it("renders leading and meta nodes", () => {
    render(
      <ActionToast
        title="Title"
        leading={<span data-testid="leading-icon">*</span>}
        meta={<span data-testid="meta-text">2m ago</span>}
      />,
    );
    expect(screen.getByTestId("leading-icon")).toBeVisible();
    expect(screen.getByTestId("meta-text")).toBeVisible();
  });

  it("gives accent actions the accent className", () => {
    render(
      <ActionToast
        title="Title"
        actions={[{ label: "Open", onClick: () => {}, accent: true }]}
      />,
    );
    expect(screen.getByRole("button", { name: "Open" })).toHaveClass(
      "text-[var(--accent-primary)]",
    );
  });
});
