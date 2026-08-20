import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { IntentStep } from "./IntentStep";

describe("IntentStep", () => {
  it("renders Clone, Create New, and Add Existing in that order", () => {
    render(<IntentStep onSelectIntent={vi.fn()} />);
    const cards = screen.getAllByRole("radio");
    expect(cards).toHaveLength(3);
    expect(screen.getByTestId("intent-clone")).toBeInTheDocument();
    expect(screen.getByTestId("intent-create")).toBeInTheDocument();
    expect(screen.getByTestId("intent-existing")).toBeInTheDocument();
  });

  it("shares one radio group name for native arrow-key navigation", () => {
    render(<IntentStep onSelectIntent={vi.fn()} />);
    for (const testId of ["intent-clone", "intent-create", "intent-existing"]) {
      const input = screen.getByTestId(testId).querySelector("input[type='radio']");
      expect(input).toHaveAttribute("name", "project-intent");
    }
  });

  it("selecting a card calls onSelectIntent with the matching step", async () => {
    const user = userEvent.setup();
    const onSelectIntent = vi.fn();
    render(<IntentStep onSelectIntent={onSelectIntent} />);

    await user.click(screen.getByTestId("intent-create"));
    expect(onSelectIntent).toHaveBeenCalledWith("create");

    await user.click(screen.getByTestId("intent-existing"));
    expect(onSelectIntent).toHaveBeenCalledWith("existing");

    await user.click(screen.getByTestId("intent-clone"));
    expect(onSelectIntent).toHaveBeenCalledWith("clone");
  });
});
