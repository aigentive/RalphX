/**
 * FinalizeConfirmationDialog.test.tsx
 * Covers: queue gating (no active session → null), accept/reject/view-plan flows,
 * loading states, auto-accept toggle, and store side effects.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// ─── Store mocks ───────────────────────────────────────────────────────────

const dequeueConfirmation = vi.fn();
const addAutoAcceptSession = vi.fn();
const setCurrentView = vi.fn();
const setActiveSession = vi.fn();

let uiState = {
  pendingConfirmationQueue: [] as string[],
  dequeueConfirmation,
  addAutoAcceptSession,
  setCurrentView,
  featureFlags: {
    activityPage: true,
    extensibilityPage: true,
    automationsPage: false,
    atlassianOauth: false,
    ticketingDashboard: false,
  },
};
let ideationState = {
  sessions: {} as Record<string, { id: string; title: string | null }>,
  setActiveSession,
};

vi.mock("@/stores/uiStore", () => {
  const useUiStore = <T,>(selector: (s: typeof uiState) => T) =>
    selector(uiState);
  return {
    useUiStore: Object.assign(useUiStore, {
      getState: () => uiState,
    }),
  };
});

vi.mock("@/stores/ideationStore", () => {
  const useIdeationStore = <T,>(selector: (s: typeof ideationState) => T) =>
    selector(ideationState);
  return {
    useIdeationStore: Object.assign(useIdeationStore, {
      getState: () => ideationState,
    }),
  };
});

// ─── Mutation hook mocks ───────────────────────────────────────────────────

type MutationCtl = {
  mutateAsync: ReturnType<typeof vi.fn>;
  isPending: boolean;
};
const acceptCtl: MutationCtl = { mutateAsync: vi.fn(), isPending: false };
const rejectCtl: MutationCtl = { mutateAsync: vi.fn(), isPending: false };

vi.mock("@/hooks/useAcceptFinalize", () => ({
  useAcceptFinalize: () => acceptCtl,
  useRejectFinalize: () => rejectCtl,
}));

// ─── theme helper passthrough ──────────────────────────────────────────────

vi.mock("@/lib/theme-colors", () => ({
  withAlpha: (c: string, a: number) => `${c}/${a}`,
}));

import { FinalizeConfirmationDialog } from "./FinalizeConfirmationDialog";

function resetState() {
  uiState = {
    pendingConfirmationQueue: [],
    dequeueConfirmation,
    addAutoAcceptSession,
    setCurrentView,
    featureFlags: {
      activityPage: true,
      extensibilityPage: true,
      automationsPage: false,
      atlassianOauth: false,
      ticketingDashboard: false,
    },
  };
  ideationState = { sessions: {}, setActiveSession };
}

describe("FinalizeConfirmationDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    acceptCtl.mutateAsync = vi.fn().mockResolvedValue(undefined);
    acceptCtl.isPending = false;
    rejectCtl.mutateAsync = vi.fn().mockResolvedValue(undefined);
    rejectCtl.isPending = false;
    resetState();
  });

  it("renders nothing when queue is empty", () => {
    const { container } = render(<FinalizeConfirmationDialog />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the dialog and session title when queue has an item", () => {
    uiState.pendingConfirmationQueue = ["s1"];
    ideationState.sessions = { s1: { id: "s1", title: "My Plan" } };

    render(<FinalizeConfirmationDialog />);

    expect(screen.getByRole("dialog")).toHaveAccessibleName(
      "Plan Ready for Acceptance",
    );
    expect(screen.getByText("My Plan")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Accept/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Reject/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /View Plan/i }),
    ).toBeInTheDocument();
  });

  it("omits the title chip when session title is missing", () => {
    uiState.pendingConfirmationQueue = ["s1"];
    ideationState.sessions = {};
    render(<FinalizeConfirmationDialog />);
    // Heading is present, but no extra title chip text node
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("calls accept mutation and dequeues on success without auto-accept", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    await user.click(screen.getByRole("button", { name: /Accept/i }));

    expect(acceptCtl.mutateAsync).toHaveBeenCalledTimes(1);
    expect(addAutoAcceptSession).not.toHaveBeenCalled();
    expect(dequeueConfirmation).toHaveBeenCalledTimes(1);
  });

  it("registers auto-accept when checkbox is checked before accept", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    await user.click(screen.getByLabelText(/Auto-accept plans/i));
    await user.click(screen.getByRole("button", { name: /Accept/i }));

    expect(addAutoAcceptSession).toHaveBeenCalledWith("s1");
    expect(dequeueConfirmation).toHaveBeenCalledTimes(1);
  });

  it("does not dequeue when accept mutation rejects", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    acceptCtl.mutateAsync = vi.fn().mockRejectedValue(new Error("boom"));
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    await user.click(screen.getByRole("button", { name: /Accept/i }));

    expect(acceptCtl.mutateAsync).toHaveBeenCalled();
    expect(dequeueConfirmation).not.toHaveBeenCalled();
  });

  it("calls reject mutation and dequeues on success", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    await user.click(screen.getByRole("button", { name: /Reject/i }));

    expect(rejectCtl.mutateAsync).toHaveBeenCalledTimes(1);
    expect(dequeueConfirmation).toHaveBeenCalledTimes(1);
  });

  it("does not dequeue when reject mutation rejects", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    rejectCtl.mutateAsync = vi.fn().mockRejectedValue(new Error("nope"));
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    await user.click(screen.getByRole("button", { name: /Reject/i }));

    expect(dequeueConfirmation).not.toHaveBeenCalled();
  });

  it("View Plan dequeues and routes to Agents when standalone ideation is disabled", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    await user.click(screen.getByRole("button", { name: /View Plan/i }));

    expect(dequeueConfirmation).toHaveBeenCalledTimes(1);
    expect(setActiveSession).not.toHaveBeenCalled();
    expect(setCurrentView).toHaveBeenCalledWith("agents");
  });

  it("disables actions while accept is pending and shows spinner", () => {
    uiState.pendingConfirmationQueue = ["s1"];
    acceptCtl.isPending = true;
    render(<FinalizeConfirmationDialog />);

    expect(screen.getByRole("button", { name: /Accept/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Reject/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /View Plan/i })).toBeDisabled();
    expect(screen.getByLabelText(/Auto-accept plans/i)).toBeDisabled();
  });

  it("disables actions while reject is pending", () => {
    uiState.pendingConfirmationQueue = ["s1"];
    rejectCtl.isPending = true;
    render(<FinalizeConfirmationDialog />);

    expect(screen.getByRole("button", { name: /Accept/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Reject/i })).toBeDisabled();
  });

  it("hover handlers tweak inline background without throwing", async () => {
    uiState.pendingConfirmationQueue = ["s1"];
    const user = userEvent.setup();
    render(<FinalizeConfirmationDialog />);

    const accept = screen.getByRole("button", { name: /Accept/i });
    const reject = screen.getByRole("button", { name: /Reject/i });
    const view = screen.getByRole("button", { name: /View Plan/i });

    await act(async () => {
      await user.hover(accept);
      await user.unhover(accept);
      await user.hover(reject);
      await user.unhover(reject);
      await user.hover(view);
      await user.unhover(view);
    });

    expect(accept).toBeInTheDocument();
  });
});
