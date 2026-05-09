import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { VerificationHistory } from "./VerificationHistory";
import type { RoundSummary, VerificationGap, VerificationRoundDetail } from "@/types/ideation";

const mockRounds: RoundSummary[] = [
  { round: 1, gapScore: 23, gapCount: 4 },
  { round: 2, gapScore: 10, gapCount: 2 },
  { round: 3, gapScore: 0, gapCount: 0 },
];

const mockGaps: VerificationGap[] = [
  { severity: "high", category: "correctness", description: "Missing null check", whyItMatters: "Will crash" },
  { severity: "medium", category: "performance", description: "No caching" },
  { severity: "low", category: "style", description: "Inconsistent naming" },
];

const mockRoundDetails: VerificationRoundDetail[] = [
  {
    round: 1,
    gapScore: 13,
    gapCount: 2,
    gaps: [
      { severity: "critical", category: "completeness", description: "Missing migration registration" },
      { severity: "high", category: "testing", description: "Missing register-project coverage" },
    ],
  },
  {
    round: 2,
    gapScore: 3,
    gapCount: 1,
    gaps: [
      { severity: "high", category: "testing", description: "Missing register-project coverage" },
    ],
  },
];

describe("VerificationHistory", () => {
  it("renders round score trend bars", () => {
    render(<VerificationHistory rounds={mockRounds} />);

    // Gap scores visible as labels above bars
    expect(screen.getByText("23")).toBeInTheDocument();
    expect(screen.getByText("10")).toBeInTheDocument();
    expect(screen.getByText("0")).toBeInTheDocument();

    // Round labels
    expect(screen.getByText("R1")).toBeInTheDocument();
    expect(screen.getByText("R2")).toBeInTheDocument();
    expect(screen.getByText("R3")).toBeInTheDocument();
  });

  it("renders gap score by round section heading when rounds provided", () => {
    render(<VerificationHistory rounds={mockRounds} />);
    expect(screen.getByText(/Gap Score by Round/i)).toBeInTheDocument();
  });

  it("renders empty state when no rounds and no gaps", () => {
    render(<VerificationHistory rounds={[]} />);
    expect(screen.getByText(/No verification rounds recorded/i)).toBeInTheDocument();
  });

  it("renders current gaps breakdown grouped by severity", () => {
    render(<VerificationHistory rounds={[]} currentGaps={mockGaps} />);

    expect(screen.getByText(/Final Gaps \(3\)/i)).toBeInTheDocument();
    expect(screen.getByText("Missing null check")).toBeInTheDocument();
    expect(screen.getByText("No caching")).toBeInTheDocument();
    expect(screen.getByText("Inconsistent naming")).toBeInTheDocument();
  });

  it("shows gap severity labels in breakdown", () => {
    render(<VerificationHistory rounds={[]} currentGaps={mockGaps} />);

    expect(screen.getByText(/High \(1\)/i)).toBeInTheDocument();
    expect(screen.getByText(/Medium \(1\)/i)).toBeInTheDocument();
    expect(screen.getByText(/Low \(1\)/i)).toBeInTheDocument();
  });

  it("shows whyItMatters when provided", () => {
    render(<VerificationHistory rounds={[]} currentGaps={mockGaps} />);
    expect(screen.getByText("Will crash")).toBeInTheDocument();
  });

  it("does not show critical label when no critical gaps", () => {
    render(<VerificationHistory rounds={[]} currentGaps={mockGaps} />);
    // mockGaps has no critical severity
    expect(screen.queryByText(/Critical/)).not.toBeInTheDocument();
  });

  it("renders round lineage with addressed gaps for the latest round by default", async () => {
    const user = userEvent.setup();

    render(
      <VerificationHistory
        rounds={mockRounds}
        roundDetails={mockRoundDetails}
        currentGaps={mockRoundDetails[1]?.gaps}
      />
    );

    expect(screen.getByText(/Round Lineage/i)).toBeInTheDocument();

    // Latest round (R2) is expanded by default — gaps and addressed toggle visible.
    expect(screen.getByText("Missing register-project coverage")).toBeInTheDocument();
    expect(screen.getByText(/1 addressed since round 1/i)).toBeInTheDocument();
    // Addressed gaps still collapsed by default within the latest round.
    expect(screen.queryByText("Missing migration registration")).not.toBeInTheDocument();

    // Expand the addressed gaps disclosure.
    await user.click(screen.getByText(/1 addressed since round 1/i));
    expect(screen.getByText("Missing migration registration")).toBeInTheDocument();
  });

  it("only renders the selected round in the lineage list", () => {
    render(
      <VerificationHistory
        rounds={mockRounds}
        roundDetails={mockRoundDetails}
      />
    );

    // Default selection = latest round (R2). R1 should not appear in the lineage body.
    const lineageHeading = screen.getByText(/Round Lineage/i);
    const lineageContainer = lineageHeading.parentElement!;
    expect(lineageContainer.textContent).toContain("R2");
    expect(lineageContainer.textContent).not.toMatch(/\bR1\b.*1 fixed|0 fixed/);
  });

  it("clicking a chart bar selects that round's lineage", async () => {
    const user = userEvent.setup();

    render(
      <VerificationHistory
        rounds={mockRounds}
        roundDetails={mockRoundDetails}
      />
    );

    // Latest round R2 expanded by default — its gap visible. R1's
    // exclusive gap should not be on screen yet.
    expect(screen.getByText("Missing register-project coverage")).toBeInTheDocument();
    expect(screen.queryByText("Missing migration registration")).not.toBeInTheDocument();

    // Click the R1 bar — lineage swaps to round 1, exposing R1's gap.
    await user.click(screen.getByTestId("verification-round-bar-1"));

    expect(screen.getByText("Missing migration registration")).toBeInTheDocument();
  });
});
