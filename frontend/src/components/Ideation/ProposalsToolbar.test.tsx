/**
 * Tests for ProposalsToolbar — covers count copy, conditional buttons,
 * disabled/enabled accept logic, label variants, and warning icons.
 */

import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TaskProposal } from "@/types/ideation";
import type { DependencyGraphResponse } from "@/api/ideation.types";

const { mocks } = vi.hoisted(() => ({
  mocks: {
    validation: {
      isComplete: true,
      message: "All dependencies analyzed",
    } as { isComplete: boolean; message: string },
    gate: {
      canAccept: true,
      reason: "",
    } as { canAccept: boolean; reason: string },
  },
}));

vi.mock("@/hooks/useDependencyGraphComplete", () => ({
  useDependencyGraphValidation: () => mocks.validation,
}));

vi.mock("@/hooks/useVerificationGate", () => ({
  useVerificationGate: () => mocks.gate,
}));

vi.mock("@/lib/theme-colors", () => ({
  withAlpha: (color: string, alpha: number) => `${color}/${alpha}`,
}));

import { ProposalsToolbar } from "./ProposalsToolbar";

const makeProposal = (id: string): TaskProposal =>
  ({
    id,
    title: `Proposal ${id}`,
  }) as unknown as TaskProposal;

const proposals = (n: number) =>
  Array.from({ length: n }, (_, i) => makeProposal(String(i + 1)));

const baseProps = () => ({
  proposals: proposals(2),
  graph: null as DependencyGraphResponse | null,
  onClearAll: vi.fn(),
  onAcceptPlan: vi.fn(),
  onAnalyzeDependencies: vi.fn(),
});

beforeEach(() => {
  mocks.validation = { isComplete: true, message: "All dependencies analyzed" };
  mocks.gate = { canAccept: true, reason: "" };
});

describe("ProposalsToolbar", () => {
  it("renders 'proposals' (plural) for multiple items", () => {
    render(<ProposalsToolbar {...baseProps()} />);
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText(/proposals/)).toBeInTheDocument();
  });

  it("renders 'proposal' (singular) for one item", () => {
    render(<ProposalsToolbar {...baseProps()} proposals={proposals(1)} />);
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText(/^\s*proposal\s*$/)).toBeInTheDocument();
  });

  it("shows analyze and clear buttons when not read-only and 2+ proposals", () => {
    const props = baseProps();
    render(<ProposalsToolbar {...props} />);
    const buttons = screen.getAllByRole("button");
    // analyze + clear + accept = 3
    expect(buttons.length).toBeGreaterThanOrEqual(3);
  });

  it("hides analyze button when fewer than 2 proposals", () => {
    const props = baseProps();
    render(<ProposalsToolbar {...props} proposals={proposals(1)} />);
    // Only clear + accept = 2 buttons
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(2);
  });

  it("hides analyze button when onAnalyzeDependencies is undefined", () => {
    const props = { ...baseProps(), onAnalyzeDependencies: undefined };
    render(<ProposalsToolbar {...props} />);
    const buttons = screen.getAllByRole("button");
    // clear + accept = 2 (no analyze)
    expect(buttons).toHaveLength(2);
  });

  it("hides analyze, clear, and accept buttons when isReadOnly", () => {
    render(<ProposalsToolbar {...baseProps()} isReadOnly />);
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("calls onClearAll when clear button is clicked", async () => {
    const user = userEvent.setup();
    const onClearAll = vi.fn();
    render(<ProposalsToolbar {...baseProps()} onClearAll={onClearAll} />);
    // Clear is the second button (after analyze)
    const buttons = screen.getAllByRole("button");
    // Find clear by its trash icon button (between analyze and accept).
    // There are 3 buttons: [analyze, clear, accept]
    await user.click(buttons[1]!);
    expect(onClearAll).toHaveBeenCalledTimes(1);
  });

  it("calls onAnalyzeDependencies when analyze button is clicked", async () => {
    const user = userEvent.setup();
    const onAnalyzeDependencies = vi.fn();
    render(
      <ProposalsToolbar
        {...baseProps()}
        onAnalyzeDependencies={onAnalyzeDependencies}
      />,
    );
    const buttons = screen.getAllByRole("button");
    await user.click(buttons[0]!);
    expect(onAnalyzeDependencies).toHaveBeenCalledTimes(1);
  });

  it("renders Accept Plan with count when graph complete and gate open", () => {
    render(<ProposalsToolbar {...baseProps()} />);
    expect(screen.getByText("Accept Plan (2)")).toBeInTheDocument();
  });

  it("renders 'Accept without dependencies' when analysisTimedOut is true", () => {
    render(<ProposalsToolbar {...baseProps()} analysisTimedOut />);
    expect(
      screen.getByText("Accept without dependencies"),
    ).toBeInTheDocument();
  });

  it("calls onAcceptPlan when accept button is clicked and enabled", async () => {
    const user = userEvent.setup();
    const onAcceptPlan = vi.fn();
    render(
      <ProposalsToolbar {...baseProps()} onAcceptPlan={onAcceptPlan} />,
    );
    await user.click(screen.getByText("Accept Plan (2)"));
    expect(onAcceptPlan).toHaveBeenCalledTimes(1);
  });

  it("disables accept button when totalCount is 0", () => {
    render(<ProposalsToolbar {...baseProps()} proposals={[]} />);
    const acceptBtn = screen.getByText("Accept Plan (0)").closest("button");
    expect(acceptBtn).toBeDisabled();
  });

  it("disables accept button when graph validation is incomplete", () => {
    mocks.validation = { isComplete: false, message: "Missing edges" };
    render(<ProposalsToolbar {...baseProps()} />);
    const acceptBtn = screen.getByText("Accept Plan (2)").closest("button");
    expect(acceptBtn).toBeDisabled();
  });

  it("leaves acceptance to the backend verification policy", () => {
    mocks.gate = { canAccept: false, reason: "Verification required" };
    render(<ProposalsToolbar {...baseProps()} />);
    const acceptBtn = screen.getByText("Accept Plan (2)").closest("button");
    expect(acceptBtn).not.toBeDisabled();
  });

  it("disables accept button when isPendingAcceptance is true", () => {
    render(<ProposalsToolbar {...baseProps()} isPendingAcceptance />);
    const acceptBtn = screen.getByText("Accept Plan (2)").closest("button");
    expect(acceptBtn).toBeDisabled();
  });

  it("renders graph-incomplete warning icon when validation is incomplete", () => {
    mocks.validation = { isComplete: false, message: "Missing edges" };
    const { container } = render(<ProposalsToolbar {...baseProps()} />);
    // AlertCircle icon — find via lucide class
    const alerts = container.querySelectorAll(".lucide-alert-circle, .lucide-circle-alert");
    expect(alerts.length).toBeGreaterThanOrEqual(1);
  });

  it("does not render graph-incomplete warning when read-only", () => {
    mocks.validation = { isComplete: false, message: "Missing edges" };
    const { container } = render(
      <ProposalsToolbar {...baseProps()} isReadOnly />,
    );
    const alerts = container.querySelectorAll(".lucide-alert-circle, .lucide-circle-alert");
    expect(alerts.length).toBe(0);
  });

  it("renders pending-acceptance warning when isPendingAcceptance and not readonly", () => {
    const { container } = render(
      <ProposalsToolbar {...baseProps()} isPendingAcceptance />,
    );
    const alerts = container.querySelectorAll(".lucide-alert-circle, .lucide-circle-alert");
    expect(alerts.length).toBeGreaterThanOrEqual(1);
  });

  it("does not render a stale client-side verification gate warning", () => {
    mocks.gate = { canAccept: false, reason: "Verification required" };
    const { container } = render(<ProposalsToolbar {...baseProps()} />);
    const shields = container.querySelectorAll(".lucide-shield-alert");
    expect(shields.length).toBe(0);
  });

  it("does not render verification-blocked shield when totalCount is 0", () => {
    mocks.gate = { canAccept: false, reason: "Verification required" };
    const { container } = render(
      <ProposalsToolbar {...baseProps()} proposals={[]} />,
    );
    const shields = container.querySelectorAll(".lucide-shield-alert");
    expect(shields.length).toBe(0);
  });

  it("applies hover styles on analyze button mouseEnter and clears on mouseLeave", () => {
    render(<ProposalsToolbar {...baseProps()} />);
    const buttons = screen.getAllByRole("button");
    const analyzeBtn = buttons[0]!;
    fireEvent.mouseEnter(analyzeBtn);
    expect(analyzeBtn.style.color).not.toBe("");
    fireEvent.mouseLeave(analyzeBtn);
    expect(analyzeBtn.style.background).toBe("transparent");
  });

  it("applies hover styles on clear button mouseEnter and clears on mouseLeave", () => {
    render(<ProposalsToolbar {...baseProps()} />);
    const buttons = screen.getAllByRole("button");
    const clearBtn = buttons[1]!;
    fireEvent.mouseEnter(clearBtn);
    expect(clearBtn.style.color).not.toBe("");
    fireEvent.mouseLeave(clearBtn);
    expect(clearBtn.style.background).toBe("transparent");
  });

  it("applies accent hover background on accept button when canAccept", () => {
    render(<ProposalsToolbar {...baseProps()} />);
    const acceptBtn = screen.getByText("Accept Plan (2)").closest("button")!;
    fireEvent.mouseEnter(acceptBtn);
    expect(acceptBtn.style.background).toContain("accent-muted");
    fireEvent.mouseLeave(acceptBtn);
    // mouseLeave restores withAlpha-formatted background
    expect(acceptBtn.style.background).not.toBe("");
  });

  it("does not change accept hover background when accept is disabled", () => {
    mocks.validation = { isComplete: false, message: "Missing edges" };
    render(<ProposalsToolbar {...baseProps()} />);
    const acceptBtn = screen.getByText("Accept Plan (2)").closest("button")!;
    const before = acceptBtn.style.background;
    fireEvent.mouseEnter(acceptBtn);
    expect(acceptBtn.style.background).toBe(before);
    fireEvent.mouseLeave(acceptBtn);
    expect(acceptBtn.style.background).toBe(before);
  });
});
