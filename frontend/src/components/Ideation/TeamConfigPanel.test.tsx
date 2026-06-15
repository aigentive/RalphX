/**
 * TeamConfigPanel component tests
 *
 * Tests max teammates select, model ceiling, budget conversion,
 * and composition mode radio behavior.
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TeamConfigPanel } from "./TeamConfigPanel";
import type { TeamConfig } from "@/types/ideation";

const DEFAULT_CONFIG: TeamConfig = {
  maxTeammates: 5,
  modelCeiling: "sonnet",
  compositionMode: "dynamic",
};

function renderPanel(config?: Partial<TeamConfig>, onChange = vi.fn()) {
  const merged: TeamConfig = { ...DEFAULT_CONFIG, ...config };
  return { onChange, ...render(<TeamConfigPanel config={merged} onChange={onChange} />) };
}

describe("TeamConfigPanel", () => {
  describe("max teammates select", () => {
    it("renders options from 2 to 8", () => {
      renderPanel();
      const selects = screen.getAllByRole("combobox");
      // First select is max teammates
      const maxSelect = selects[0]!;
      const options = maxSelect.querySelectorAll("option");
      const values = Array.from(options).map((o) => Number(o.getAttribute("value")));
      expect(values).toEqual([2, 3, 4, 5, 6, 7, 8]);
    });

    it("reflects current config value", () => {
      renderPanel({ maxTeammates: 3 });
      const selects = screen.getAllByRole("combobox");
      expect(selects[0]).toHaveValue("3");
    });

    it("calls onChange with updated maxTeammates on change", () => {
      const onChange = vi.fn();
      renderPanel({ maxTeammates: 5 }, onChange);
      const selects = screen.getAllByRole("combobox");
      fireEvent.change(selects[0]!, { target: { value: "8" } });
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ maxTeammates: 8 }),
      );
    });
  });

  describe("model ceiling select", () => {
    it("renders Haiku, Sonnet, Opus, and Fable options", () => {
      renderPanel();
      expect(screen.getByText("Haiku")).toBeInTheDocument();
      expect(screen.getByText("Sonnet")).toBeInTheDocument();
      expect(screen.getByText("Opus")).toBeInTheDocument();
      expect(screen.getByText("Fable")).toBeInTheDocument();
    });

    it("reflects current modelCeiling value", () => {
      renderPanel({ modelCeiling: "opus" });
      const selects = screen.getAllByRole("combobox");
      // Second select is model ceiling
      expect(selects[1]).toHaveValue("opus");
    });

    it("calls onChange with updated modelCeiling", () => {
      const onChange = vi.fn();
      renderPanel({ modelCeiling: "sonnet" }, onChange);
      const selects = screen.getAllByRole("combobox");
      fireEvent.change(selects[1]!, { target: { value: "haiku" } });
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ modelCeiling: "haiku" }),
      );
    });
  });

  describe("budget limit conversion", () => {
    it("converts empty string to undefined", () => {
      const onChange = vi.fn();
      renderPanel({ budgetLimit: 10 }, onChange);
      const selects = screen.getAllByRole("combobox");
      // Third select is budget
      fireEvent.change(selects[2]!, { target: { value: "" } });
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ budgetLimit: undefined }),
      );
    });

    it("converts numeric string to number", () => {
      const onChange = vi.fn();
      renderPanel(undefined, onChange);
      const selects = screen.getAllByRole("combobox");
      fireEvent.change(selects[2]!, { target: { value: "25" } });
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ budgetLimit: 25 }),
      );
    });

    it("displays current budget value", () => {
      renderPanel({ budgetLimit: 10 });
      const selects = screen.getAllByRole("combobox");
      expect(selects[2]).toHaveValue("10");
    });
  });

  describe("composition mode", () => {
    it("shows Dynamic and Constrained radio options", () => {
      renderPanel();
      expect(screen.getByText("Dynamic")).toBeInTheDocument();
      expect(screen.getByText("Constrained")).toBeInTheDocument();
    });

    it("calls onChange with compositionMode=constrained when clicked", () => {
      const onChange = vi.fn();
      renderPanel({ compositionMode: "dynamic" }, onChange);
      fireEvent.click(screen.getByText("Constrained"));
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({ compositionMode: "constrained" }),
      );
    });

    it("reveals specialist roles when constrained is selected", () => {
      renderPanel({ compositionMode: "constrained" });
      expect(screen.getByText(/researcher/)).toBeInTheDocument();
      expect(screen.getByText(/critic/)).toBeInTheDocument();
      expect(screen.getByText(/Available specialist roles:/)).toBeInTheDocument();
    });

    it("hides specialist roles when dynamic is selected", () => {
      renderPanel({ compositionMode: "dynamic" });
      expect(screen.queryByText(/Available specialist roles:/)).not.toBeInTheDocument();
    });
  });
});
