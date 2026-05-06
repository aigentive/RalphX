import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VerificationGapList } from "./VerificationGapList";
import type { RoundSummary, VerificationGap } from "@/types/ideation";

const mixedGaps: VerificationGap[] = [
  {
    severity: "critical",
    category: "completeness",
    description: "Missing migration registration",
    whyItMatters: "Schema breaks on launch",
  },
  {
    severity: "high",
    category: "correctness",
    description: "Missing null check",
    whyItMatters: "Will crash on edge case",
  },
  { severity: "medium", category: "performance", description: "No caching layer" },
  { severity: "low", category: "style", description: "Inconsistent naming" },
  { severity: "high", category: "testing", description: "Missing register-project coverage" },
];

describe("VerificationGapList", () => {
  it("renders the empty state when no gaps are provided", () => {
    render(<VerificationGapList gaps={[]} />);
    expect(screen.getByText(/No gaps found/i)).toBeInTheDocument();
  });

  it("renders gaps grouped by severity with summary counts", () => {
    render(<VerificationGapList gaps={mixedGaps} />);

    // Group section headers (uppercase labels)
    expect(screen.getAllByText("Critical").length).toBeGreaterThan(0);
    expect(screen.getAllByText("High").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Medium").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Low").length).toBeGreaterThan(0);

    // Summary chips include counts
    expect(screen.getByText(/1 Critical/)).toBeInTheDocument();
    expect(screen.getByText(/2 High/)).toBeInTheDocument();
    expect(screen.getByText(/1 Medium/)).toBeInTheDocument();
    expect(screen.getByText(/1 Low/)).toBeInTheDocument();

    // Gap descriptions render
    expect(screen.getByText("Missing migration registration")).toBeInTheDocument();
    expect(screen.getByText("Missing null check")).toBeInTheDocument();
    expect(screen.getByText("No caching layer")).toBeInTheDocument();
    expect(screen.getByText("Inconsistent naming")).toBeInTheDocument();
    expect(screen.getByText("Missing register-project coverage")).toBeInTheDocument();
  });

  it("renders whyItMatters and category metadata", () => {
    render(<VerificationGapList gaps={mixedGaps} />);

    expect(screen.getByText("Schema breaks on launch")).toBeInTheDocument();
    expect(screen.getByText("Will crash on edge case")).toBeInTheDocument();
    expect(screen.getByText("completeness")).toBeInTheDocument();
    expect(screen.getByText("correctness")).toBeInTheDocument();
    expect(screen.getByText("performance")).toBeInTheDocument();
    expect(screen.getByText("style")).toBeInTheDocument();
  });

  it("renders gap score when provided without rounds", () => {
    render(<VerificationGapList gaps={mixedGaps} gapScore={17} />);
    expect(screen.getByText(/Gap score:/i)).toBeInTheDocument();
    expect(screen.getByText("17")).toBeInTheDocument();
  });

  it("does not render gap score row when neither rounds nor gapScore provided", () => {
    render(<VerificationGapList gaps={mixedGaps} />);
    expect(screen.queryByText(/Gap score:/i)).not.toBeInTheDocument();
  });

  it("renders gap score trend with TrendingDown when score improves", () => {
    const rounds: RoundSummary[] = [
      { round: 1, gapScore: 20, gapCount: 4 },
      { round: 2, gapScore: 5, gapCount: 1 },
    ];
    render(<VerificationGapList gaps={mixedGaps} rounds={rounds} />);

    expect(screen.getByText(/Gap score:/i)).toBeInTheDocument();
    expect(screen.getByText("5")).toBeInTheDocument();
    expect(screen.getByText(/−15 from round 1/)).toBeInTheDocument();
  });

  it("renders gap score trend with TrendingUp when score worsens", () => {
    const rounds: RoundSummary[] = [
      { round: 1, gapScore: 5, gapCount: 1 },
      { round: 2, gapScore: 12, gapCount: 3 },
    ];
    render(<VerificationGapList gaps={mixedGaps} rounds={rounds} />);

    expect(screen.getByText(/\+7 from round 1/)).toBeInTheDocument();
  });

  it("renders gap score trend with stable indicator when delta is zero", () => {
    const rounds: RoundSummary[] = [
      { round: 1, gapScore: 8, gapCount: 2 },
      { round: 2, gapScore: 8, gapCount: 2 },
    ];
    render(<VerificationGapList gaps={mixedGaps} rounds={rounds} />);

    expect(screen.getByText(/No change/i)).toBeInTheDocument();
  });

  it("does not render trend section when only one round is provided", () => {
    const rounds: RoundSummary[] = [{ round: 1, gapScore: 5, gapCount: 1 }];
    render(<VerificationGapList gaps={mixedGaps} rounds={rounds} />);

    expect(screen.queryByText(/from round/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/No change/i)).not.toBeInTheDocument();
  });

  it("renders mini round history bars (max 5) with title attributes", () => {
    const rounds: RoundSummary[] = [
      { round: 1, gapScore: 30, gapCount: 6 },
      { round: 2, gapScore: 25, gapCount: 5 },
      { round: 3, gapScore: 18, gapCount: 4 },
      { round: 4, gapScore: 10, gapCount: 2 },
      { round: 5, gapScore: 5, gapCount: 1 },
      { round: 6, gapScore: 2, gapCount: 1 },
    ];
    render(<VerificationGapList gaps={mixedGaps} rounds={rounds} />);

    // Latest round only (shows in trend label)
    expect(screen.getByTitle(/Round 6: score 2/)).toBeInTheDocument();
    // Round 1 should be off the slice -5
    expect(screen.queryByTitle(/Round 1: score 30/)).not.toBeInTheDocument();
    expect(screen.getByTitle(/Round 2: score 25/)).toBeInTheDocument();
  });

  it("does not show Select All button when not selectable", () => {
    render(<VerificationGapList gaps={mixedGaps} />);
    expect(screen.queryByRole("button", { name: /Select All/i })).not.toBeInTheDocument();
  });

  it("shows Select All button when selectable + onSelectionChange provided", () => {
    const onSelectionChange = vi.fn();
    render(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={new Set()}
        onSelectionChange={onSelectionChange}
      />
    );
    expect(screen.getByRole("button", { name: /Select All/i })).toBeInTheDocument();
  });

  it("clicking Select All selects every gap index", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={new Set()}
        onSelectionChange={onSelectionChange}
      />
    );

    await user.click(screen.getByRole("button", { name: /Select All/i }));

    expect(onSelectionChange).toHaveBeenCalledTimes(1);
    const arg = onSelectionChange.mock.calls[0]![0] as Set<number>;
    expect(arg.size).toBe(mixedGaps.length);
    expect([...arg].sort()).toEqual([0, 1, 2, 3, 4]);
  });

  it("shows Deselect All when everything is selected, and clicking clears selection", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    const allSelected = new Set([0, 1, 2, 3, 4]);
    render(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={allSelected}
        onSelectionChange={onSelectionChange}
      />
    );

    const btn = screen.getByRole("button", { name: /Deselect All/i });
    await user.click(btn);

    expect(onSelectionChange).toHaveBeenCalledTimes(1);
    const arg = onSelectionChange.mock.calls[0]![0] as Set<number>;
    expect(arg.size).toBe(0);
  });

  it("clicking a gap row toggles selection on, then off", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    const { rerender } = render(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={new Set()}
        onSelectionChange={onSelectionChange}
      />
    );

    // Click critical gap row (index 0 in flat list)
    await user.click(screen.getByText("Missing migration registration"));
    expect(onSelectionChange).toHaveBeenCalledTimes(1);
    const firstArg = onSelectionChange.mock.calls[0]![0] as Set<number>;
    expect(firstArg.has(0)).toBe(true);

    // Now simulate parent updating with that selected, then clicking again to deselect
    rerender(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={new Set([0])}
        onSelectionChange={onSelectionChange}
      />
    );

    await user.click(screen.getByText("Missing migration registration"));
    const secondArg = onSelectionChange.mock.calls[1]![0] as Set<number>;
    expect(secondArg.has(0)).toBe(false);
  });

  it("does not crash when selectable but onSelectionChange is missing", () => {
    render(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={new Set([0])}
      />
    );
    expect(screen.getByText("Missing migration registration")).toBeInTheDocument();
    // No Select All since onSelectionChange is required for that affordance
    expect(screen.queryByRole("button", { name: /Select All/i })).not.toBeInTheDocument();
  });

  it("hover handlers on Select All toggle inline color without throwing", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    render(
      <VerificationGapList
        gaps={mixedGaps}
        selectable
        selectedGaps={new Set()}
        onSelectionChange={onSelectionChange}
      />
    );

    const btn = screen.getByRole("button", { name: /Select All/i });
    await user.hover(btn);
    await user.unhover(btn);
    // Reaching here without throwing is the assertion
    expect(btn).toBeInTheDocument();
  });
});
