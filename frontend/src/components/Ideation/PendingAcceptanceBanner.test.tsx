import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { PendingAcceptanceBanner } from "./PendingAcceptanceBanner";

const { acceptMutate, rejectMutate, removeFromQueue } = vi.hoisted(() => ({
  acceptMutate: vi.fn(),
  rejectMutate: vi.fn(),
  removeFromQueue: vi.fn(),
}));

vi.mock("@/hooks/useAcceptFinalize", () => ({
  useAcceptFinalize: () => ({
    mutateAsync: (...args: unknown[]) => acceptMutate(...args),
    isPending: false,
  }),
  useRejectFinalize: () => ({
    mutateAsync: (...args: unknown[]) => rejectMutate(...args),
    isPending: false,
  }),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (s: { removeFromConfirmationQueue: typeof removeFromQueue }) => unknown) =>
    selector({ removeFromConfirmationQueue: removeFromQueue }),
}));

beforeEach(() => {
  acceptMutate.mockReset();
  rejectMutate.mockReset();
  removeFromQueue.mockReset();
  acceptMutate.mockResolvedValue(undefined);
  rejectMutate.mockResolvedValue(undefined);
});

describe("PendingAcceptanceBanner", () => {
  it("renders the banner copy and action buttons", () => {
    render(<PendingAcceptanceBanner sessionId="session-1" />);
    expect(screen.getByText(/Plan pending your confirmation/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Accept/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Reject/i })).toBeInTheDocument();
  });

  it("invokes accept mutation when Accept is clicked", async () => {
    const user = userEvent.setup();
    render(<PendingAcceptanceBanner sessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: /Accept/i }));
    expect(acceptMutate).toHaveBeenCalled();
  });

  it("invokes reject mutation when Reject is clicked", async () => {
    const user = userEvent.setup();
    render(<PendingAcceptanceBanner sessionId="session-1" />);
    await user.click(screen.getByRole("button", { name: /Reject/i }));
    expect(rejectMutate).toHaveBeenCalled();
  });
});
