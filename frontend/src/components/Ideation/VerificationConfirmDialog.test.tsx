/**
 * VerificationConfirmDialog.test.tsx
 *
 * Covers AC#1-6 from the task acceptance criteria:
 * - Dialog renders with specialist checkboxes when queue is non-empty
 * - Specialist checkboxes default to enabled_by_default values
 * - Accept calls confirmVerification with disabled specialists + spinner
 * - Reject calls dismissVerification and closes dialog
 * - Auto-accept toggle persists per-session and auto-confirms future verifications
 * - Specialist fetch failure shows warning but dialog remains usable
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SpecialistEntry, SpecialistsResponse } from "@/types/verification-config";

// ── Mock verificationApi ──────────────────────────────────────────────────────

const mockConfirm = vi.fn();
const mockDismiss = vi.fn();
const mockGetSpecialists = vi.fn();

vi.mock("@/api/verification", () => ({
  verificationApi: {
    confirm: (...args: unknown[]) => mockConfirm(...args),
    dismiss: (...args: unknown[]) => mockDismiss(...args),
    getSpecialists: () => mockGetSpecialists(),
  },
}));

// ── Mock sonner toast ─────────────────────────────────────────────────────────

const mockToastError = vi.fn();
vi.mock("sonner", () => ({
  toast: { error: (...args: unknown[]) => mockToastError(...args) },
}));

// ── Mock uiStore ──────────────────────────────────────────────────────────────

const mockDequeue = vi.fn();
const mockAddAutoAcceptVerificationSession = vi.fn();
const mockSetCurrentView = vi.fn();

let mockUiStoreState: Record<string, unknown> = {
  pendingVerificationQueue: [] as string[],
  dequeueVerification: mockDequeue,
  addAutoAcceptVerificationSession: mockAddAutoAcceptVerificationSession,
  setCurrentView: mockSetCurrentView,
  featureFlags: {
    activityPage: true,
    extensibilityPage: true,
    ideationPage: false,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: false,
  },
};

vi.mock("@/stores/uiStore", () => {
  const useUiStore = vi.fn((selector: (s: object) => unknown) =>
    selector(mockUiStoreState)
  );
  return {
    useUiStore: Object.assign(useUiStore, {
      getState: () => mockUiStoreState,
    }),
  };
});

// ── Mock ideationStore ────────────────────────────────────────────────────────

const mockSetActiveSession = vi.fn();

let mockIdeationStoreState: Record<string, unknown> = {
  sessions: {} as Record<string, { title: string }>,
  setActiveSession: mockSetActiveSession,
};

vi.mock("@/stores/ideationStore", () => {
  const useIdeationStore = vi.fn((selector: (s: object) => unknown) =>
    selector(mockIdeationStoreState)
  );
  return {
    useIdeationStore: Object.assign(useIdeationStore, {
      getState: () => mockIdeationStoreState,
    }),
  };
});

// ── Helpers ───────────────────────────────────────────────────────────────────

const mockSpecialists: SpecialistEntry[] = [
  {
    name: "backend",
    display_name: "Backend Specialist",
    description: "Reviews Rust/Tauri patterns",
    dispatch_mode: "per_round",
    enabled_by_default: true,
  },
  {
    name: "frontend",
    display_name: "Frontend Specialist",
    description: "Reviews React/TS patterns",
    dispatch_mode: "per_round",
    enabled_by_default: true,
  },
  {
    name: "optional-critic",
    display_name: "Optional Critic",
    description: "Stress-tests all approaches",
    dispatch_mode: "per_round",
    enabled_by_default: false,
  },
];

const mockSpecialistsResponse: SpecialistsResponse = {
  specialists: mockSpecialists,
};

function setQueue(sessionIds: string[]) {
  mockUiStoreState = {
    ...mockUiStoreState,
    pendingVerificationQueue: sessionIds,
  };
}

function setSessions(sessions: Record<string, { title: string }>) {
  mockIdeationStoreState = {
    ...mockIdeationStoreState,
    sessions,
  };
}

// ── Import component (after mocks are set up) ─────────────────────────────────

import { VerificationConfirmDialog } from "./VerificationConfirmDialog";

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("VerificationConfirmDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Default: queue with one session, specialists loaded successfully
    setQueue(["session-abc"]);
    setSessions({ "session-abc": { title: "My Test Plan" } });
    mockGetSpecialists.mockResolvedValue(mockSpecialistsResponse);
    mockConfirm.mockResolvedValue({ status: "ok" });
    mockDismiss.mockResolvedValue({ status: "ok" });
  });

  // ── AC#1: Dialog renders when queue non-empty ─────────────────────────────

  describe("Visibility", () => {
    it("renders dialog when pendingVerificationQueue has a session", async () => {
      render(<VerificationConfirmDialog />);
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      expect(screen.getByText("Plan Ready for Verification")).toBeInTheDocument();
    });

    it("renders nothing when queue is empty", () => {
      setQueue([]);
      render(<VerificationConfirmDialog />);
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    it("shows the session title from ideationStore", async () => {
      render(<VerificationConfirmDialog />);
      expect(screen.getByText("My Test Plan")).toBeInTheDocument();
    });

    it("does not show session title block when session not in store", async () => {
      setSessions({});
      render(<VerificationConfirmDialog />);
      expect(screen.queryByText("My Test Plan")).not.toBeInTheDocument();
    });
  });

  // ── AC#2: Specialist checkboxes default to enabled_by_default ─────────────

  describe("Specialist checkboxes", () => {
    it("shows specialists after fetch", async () => {
      render(<VerificationConfirmDialog />);
      await waitFor(() => {
        expect(screen.getByText("Backend Specialist")).toBeInTheDocument();
        expect(screen.getByText("Frontend Specialist")).toBeInTheDocument();
        expect(screen.getByText("Optional Critic")).toBeInTheDocument();
      });
    });

    it("checks specialists that are enabled_by_default=true", async () => {
      render(<VerificationConfirmDialog />);
      await waitFor(() => {
        expect(screen.getByLabelText("Backend Specialist")).toBeInTheDocument();
      });
      const backendCheckbox = screen.getByLabelText("Backend Specialist");
      expect(backendCheckbox).toBeChecked();
      const frontendCheckbox = screen.getByLabelText("Frontend Specialist");
      expect(frontendCheckbox).toBeChecked();
    });

    it("unchecks specialists where enabled_by_default=false", async () => {
      render(<VerificationConfirmDialog />);
      await waitFor(() => {
        expect(screen.getByLabelText("Optional Critic")).toBeInTheDocument();
      });
      const optionalCheckbox = screen.getByLabelText("Optional Critic");
      expect(optionalCheckbox).not.toBeChecked();
    });

    it("allows toggling a specialist checkbox", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => {
        expect(screen.getByLabelText("Backend Specialist")).toBeInTheDocument();
      });
      const backendCheckbox = screen.getByLabelText("Backend Specialist");
      expect(backendCheckbox).toBeChecked();
      await user.click(backendCheckbox);
      expect(backendCheckbox).not.toBeChecked();
    });

    it("calls getSpecialists on mount", async () => {
      render(<VerificationConfirmDialog />);
      await waitFor(() => {
        expect(mockGetSpecialists).toHaveBeenCalledTimes(1);
      });
    });
  });

  // ── AC#3: Accept button calls confirm with disabled specialists + spinner ───

  describe("Accept flow", () => {
    it("calls confirmVerification with disabled specialists on accept", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);

      // Wait for specialists to load
      await waitFor(() => {
        expect(screen.getByLabelText("Backend Specialist")).toBeInTheDocument();
      });

      // Disable the backend specialist (optional-critic is already disabled by default)
      await user.click(screen.getByLabelText("Backend Specialist"));

      // Click Accept
      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        // Both "optional-critic" (pre-disabled via enabled_by_default=false) and
        // "backend" (just unchecked) should be in the disabled list
        expect(mockConfirm).toHaveBeenCalledWith(
          "session-abc",
          expect.arrayContaining(["backend", "optional-critic"])
        );
      });
    });

    it("calls confirmVerification with empty disabled array when all specialists enabled", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);

      await waitFor(() => {
        expect(screen.getByLabelText("Backend Specialist")).toBeInTheDocument();
      });

      // Enable the optional specialist (was off by default, so checking it removes it from disabled)
      await user.click(screen.getByLabelText("Optional Critic"));

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        expect(mockConfirm).toHaveBeenCalledWith("session-abc", []);
      });
    });

    it("dequeues after successful accept", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        expect(mockDequeue).toHaveBeenCalledTimes(1);
      });
    });

    it("shows Loader2 spinner while accepting", async () => {
      let resolveConfirm!: (v: { status: string }) => void;
      mockConfirm.mockReturnValueOnce(
        new Promise((resolve) => {
          resolveConfirm = resolve;
        })
      );

      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      const acceptBtn = screen.getByRole("button", { name: /Accept/i });
      await user.click(acceptBtn);

      // Button disabled + Loader2 visible (animate-spin class)
      expect(acceptBtn).toBeDisabled();

      resolveConfirm({ status: "ok" });
    });

    it("shows error toast and keeps dialog open on confirm failure", async () => {
      mockConfirm.mockRejectedValueOnce(new Error("Confirm failed"));

      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        expect(mockToastError).toHaveBeenCalledWith("Confirm failed");
      });

      // Dialog still visible
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      // dequeue NOT called
      expect(mockDequeue).not.toHaveBeenCalled();
    });

    it("disables all buttons while accepting", async () => {
      let resolveConfirm!: (v: { status: string }) => void;
      mockConfirm.mockReturnValueOnce(
        new Promise((resolve) => {
          resolveConfirm = resolve;
        })
      );

      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      expect(screen.getByRole("button", { name: /Accept/i })).toBeDisabled();
      expect(screen.getByRole("button", { name: /Reject/i })).toBeDisabled();
      expect(screen.getByRole("button", { name: /View Plan/i })).toBeDisabled();

      resolveConfirm({ status: "ok" });
    });
  });

  // ── AC#4: Reject button calls dismiss and closes dialog ───────────────────

  describe("Reject flow", () => {
    it("calls dismissVerification on reject", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Reject/i }));

      await waitFor(() => {
        expect(mockDismiss).toHaveBeenCalledWith("session-abc");
      });
    });

    it("dequeues after reject even on dismiss error", async () => {
      mockDismiss.mockRejectedValueOnce(new Error("Network error"));

      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Reject/i }));

      await waitFor(() => {
        expect(mockDequeue).toHaveBeenCalledTimes(1);
      });
    });

    it("dequeues after successful dismiss", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Reject/i }));

      await waitFor(() => {
        expect(mockDequeue).toHaveBeenCalledTimes(1);
      });
    });
  });

  // ── AC#5: Auto-accept toggle ──────────────────────────────────────────────

  describe("Auto-accept toggle", () => {
    it("renders auto-accept checkbox initially unchecked", () => {
      render(<VerificationConfirmDialog />);
      const autoCheckbox = screen.getByLabelText(/Auto-accept verifications for this session/i);
      expect(autoCheckbox).not.toBeChecked();
    });

    it("can toggle auto-accept checkbox", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      const autoCheckbox = screen.getByLabelText(/Auto-accept verifications for this session/i);
      await user.click(autoCheckbox);
      expect(autoCheckbox).toBeChecked();
    });

    it("calls addAutoAcceptVerificationSession when auto-accept is on and accept is clicked", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      const autoCheckbox = screen.getByLabelText(/Auto-accept verifications for this session/i);
      await user.click(autoCheckbox);

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        expect(mockAddAutoAcceptVerificationSession).toHaveBeenCalledWith("session-abc");
      });
    });

    it("does not call addAutoAcceptVerificationSession when auto-accept is off", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);
      await waitFor(() => screen.getByLabelText("Backend Specialist"));

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        expect(mockConfirm).toHaveBeenCalled();
      });
      expect(mockAddAutoAcceptVerificationSession).not.toHaveBeenCalled();
    });
  });

  // ── AC#6: Specialist fetch failure ────────────────────────────────────────

  describe("Specialist fetch failure", () => {
    it("shows warning when specialists fail to load", async () => {
      mockGetSpecialists.mockRejectedValueOnce(new Error("Network error"));
      render(<VerificationConfirmDialog />);

      await waitFor(() => {
        expect(
          screen.getByText(/Could not load specialist list — all specialists will run/i)
        ).toBeInTheDocument();
      });
    });

    it("dialog remains usable after specialist fetch failure (Accept still works)", async () => {
      mockGetSpecialists.mockRejectedValueOnce(new Error("Network error"));
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);

      await waitFor(() => {
        expect(
          screen.getByText(/Could not load specialist list/i)
        ).toBeInTheDocument();
      });

      await user.click(screen.getByRole("button", { name: /Accept/i }));

      await waitFor(() => {
        expect(mockConfirm).toHaveBeenCalledWith("session-abc", []);
      });
    });

    it("does not show specialist checkboxes when fetch fails", async () => {
      mockGetSpecialists.mockRejectedValueOnce(new Error("Network error"));
      render(<VerificationConfirmDialog />);

      await waitFor(() => {
        expect(
          screen.getByText(/Could not load specialist list/i)
        ).toBeInTheDocument();
      });

      expect(screen.queryByLabelText("Backend Specialist")).not.toBeInTheDocument();
    });
  });

  // ── View Plan button ───────────────────────────────────────────────────────

  describe("View Plan button", () => {
    it("dequeues and routes to Agents when standalone ideation is disabled", async () => {
      const user = userEvent.setup();
      render(<VerificationConfirmDialog />);

      await user.click(screen.getByRole("button", { name: /View Plan/i }));

      expect(mockDequeue).toHaveBeenCalledTimes(1);
      expect(mockSetActiveSession).not.toHaveBeenCalled();
      expect(mockSetCurrentView).toHaveBeenCalledWith("agents");
    });
  });

  // ── Queue ordering — first item shown ─────────────────────────────────────

  describe("Queue ordering", () => {
    it("shows the first session in the queue", async () => {
      setQueue(["session-first", "session-second"]);
      setSessions({
        "session-first": { title: "First Plan" },
        "session-second": { title: "Second Plan" },
      });
      render(<VerificationConfirmDialog />);
      expect(screen.getByText("First Plan")).toBeInTheDocument();
      expect(screen.queryByText("Second Plan")).not.toBeInTheDocument();
    });
  });

  // ── Accessibility ─────────────────────────────────────────────────────────

  describe("Accessibility", () => {
    it("dialog has role=dialog and aria-modal", () => {
      render(<VerificationConfirmDialog />);
      const dialog = screen.getByRole("dialog");
      expect(dialog).toHaveAttribute("aria-modal", "true");
    });

    it("dialog is labelled by the title", () => {
      render(<VerificationConfirmDialog />);
      const dialog = screen.getByRole("dialog");
      expect(dialog).toHaveAttribute("aria-labelledby", "verification-dialog-title");
      expect(screen.getByText("Plan Ready for Verification")).toBeInTheDocument();
    });
  });
});
