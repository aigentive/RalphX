import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TicketingStatePanel } from "./TicketingStatePanel";

const STATES = [
  "loading",
  "empty",
  "disconnected",
  "permission",
  "stale",
  "error",
] as const;

describe("TicketingStatePanel", () => {
  it.each(STATES)("renders the %s state with a stable testid and title", (state) => {
    render(<TicketingStatePanel state={state} title={`Title for ${state}`} />);
    expect(screen.getByTestId(`ticketing-state-${state}`)).toBeInTheDocument();
    expect(screen.getByText(`Title for ${state}`)).toBeInTheDocument();
  });

  it("renders the optional description when provided", () => {
    render(
      <TicketingStatePanel
        state="permission"
        title="Access required"
        description="You do not have permission to view these tickets."
      />,
    );
    expect(
      screen.getByText("You do not have permission to view these tickets."),
    ).toBeInTheDocument();
  });

  it("fills the available dashboard content width", () => {
    render(<TicketingStatePanel state="loading" title="Loading tickets" />);

    expect(screen.getByTestId("ticketing-state-loading").className).toContain("w-full");
  });

  it("omits the action button when only a label is provided", () => {
    render(<TicketingStatePanel state="stale" title="Data is stale" actionLabel="Refresh" />);
    expect(screen.queryByRole("button", { name: "Refresh" })).not.toBeInTheDocument();
  });

  it("omits the action button when only a handler is provided", () => {
    render(<TicketingStatePanel state="stale" title="Data is stale" onAction={vi.fn()} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("renders an action button and wires the handler when both label and handler exist", () => {
    const onAction = vi.fn();
    render(
      <TicketingStatePanel
        state="error"
        title="Something went wrong"
        actionLabel="Try again"
        onAction={onAction}
      />,
    );
    const button = screen.getByRole("button", { name: "Try again" });
    fireEvent.click(button);
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it("spins the icon only in the loading state", () => {
    const { container, rerender } = render(
      <TicketingStatePanel state="loading" title="Loading" />,
    );
    expect(container.querySelector(".animate-spin")).not.toBeNull();

    rerender(<TicketingStatePanel state="error" title="Error" />);
    expect(container.querySelector(".animate-spin")).toBeNull();
  });
});
