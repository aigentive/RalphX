import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { FieldLabel } from "./automationDetailShared";

describe("FieldLabel", () => {
  it("renders the canonical field eyebrow", () => {
    render(<FieldLabel>Branch</FieldLabel>);

    const label = screen.getByText("Branch");
    expect(label.tagName).toBe("SPAN");
    expect(label).toHaveClass(
      "text-[0.6875rem]",
      "font-semibold",
      "uppercase",
      "tracking-[0.08em]",
    );
    expect(label).toHaveStyle({ color: "var(--text-muted)" });
  });

  it("maps the group variant to secondary text with reduced opacity", () => {
    render(<FieldLabel variant="group" className="mb-2">Execution</FieldLabel>);

    const label = screen.getByText("Execution");
    expect(label).toHaveClass("mb-2", "opacity-60");
    expect(label).toHaveStyle({ color: "var(--text-secondary)" });
  });
});
