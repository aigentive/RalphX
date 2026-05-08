import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";

import { ValidationStepRow, StepsGroup } from "./ValidationProgress";
import type { MergeValidationStepEvent } from "@/types/events";

function makeStep(overrides: Partial<MergeValidationStepEvent> = {}): MergeValidationStepEvent {
  return {
    task_id: "t1",
    phase: "validate",
    command: "npm test",
    path: "/repo",
    label: "npm test",
    status: "running",
    ...overrides,
  } as MergeValidationStepEvent;
}

describe("ValidationStepRow", () => {
  it("renders the label and validate badge", () => {
    render(<ValidationStepRow step={makeStep({ label: "lint check" })} />);
    expect(screen.getByText("lint check")).toBeInTheDocument();
    expect(screen.getByText("validate")).toBeInTheDocument();
  });

  it("flags cached steps with a Cached badge", () => {
    render(<ValidationStepRow step={makeStep({ status: "cached" })} />);
    expect(screen.getByText("Cached")).toBeInTheDocument();
  });

  it("renders duration when duration_ms is set", () => {
    const { container } = render(
      <ValidationStepRow step={makeStep({ status: "success", duration_ms: 1234 })} />,
    );
    expect(container.querySelector(".lucide-clock")).toBeInTheDocument();
  });

  it("renders stdout/stderr panels when both are present", () => {
    render(
      <ValidationStepRow
        step={makeStep({ status: "failed", stdout: "out lines", stderr: "err lines" })}
      />,
    );
    expect(screen.getByText("out lines")).toBeInTheDocument();
    expect(screen.getByText("err lines")).toBeInTheDocument();
  });
});

describe("StepsGroup", () => {
  it("renders a header label and is collapsed by default for non-failed steps", () => {
    render(
      <StepsGroup
        steps={[
          makeStep({ label: "a", status: "success" }),
          makeStep({ label: "b", status: "skipped" }),
        ]}
        phase="validate"
        label="Validation"
      />,
    );
    expect(screen.getByText("Validation")).toBeInTheDocument();
    // Collapsed by default — child labels not in the DOM yet.
    expect(screen.queryByText("a")).toBeNull();
  });

  it("auto-expands when any step failed and renders child rows", () => {
    render(
      <StepsGroup
        steps={[
          makeStep({ label: "lint", status: "failed", stderr: "bad" }),
          makeStep({ label: "tests", status: "success" }),
        ]}
        phase="validate"
        label="Validation"
      />,
    );
    expect(screen.getByText("lint")).toBeInTheDocument();
    expect(screen.getByText("tests")).toBeInTheDocument();
  });
});
