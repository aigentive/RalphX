import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StatusPill } from "./status-pill";

describe("StatusPill", () => {
  it("renders the label with the neutral tone by default", () => {
    render(<StatusPill label="Pending" testId="pill" />);
    const pill = screen.getByTestId("pill");
    expect(pill).toHaveTextContent("Pending");
    expect(pill).toHaveAttribute("data-tone", "neutral");
    expect(screen.queryByTestId("pill-live-dot")).not.toBeInTheDocument();
  });

  it("applies the requested tone", () => {
    render(<StatusPill label="Running" tone="accent" testId="pill" />);
    expect(screen.getByTestId("pill")).toHaveAttribute("data-tone", "accent");
  });

  it("uses WKWebView-safe paint/border longhands", () => {
    render(<StatusPill label="Merged" tone="success" testId="pill" />);
    const pill = screen.getByTestId("pill");
    expect(pill.style.backgroundColor).not.toBe("");
    expect(pill.style.borderColor).not.toBe("");
    expect(pill.style.borderStyle).toBe("solid");
    expect(pill.style.borderWidth).toBe("1px");
  });

  it("renders a pulsing live dot when live", () => {
    render(<StatusPill label="Running" tone="accent" live testId="pill" />);
    const dot = screen.getByTestId("pill-live-dot");
    expect(dot).toHaveAttribute("aria-hidden", "true");
    expect(dot.className).toContain("animate-pulse");
  });

  it("renders an optional icon slot", () => {
    render(
      <StatusPill
        label="Done"
        tone="success"
        icon={<svg data-testid="pill-icon" />}
        testId="pill"
      />,
    );
    expect(screen.getByTestId("pill-icon")).toBeInTheDocument();
  });
});
