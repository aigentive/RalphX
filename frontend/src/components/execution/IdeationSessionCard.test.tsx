/**
 * IdeationSessionCard component tests
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { IdeationSessionCard } from "./IdeationSessionCard";
import type { RunningIdeationSession } from "@/api/running-processes";

function createMockSession(
  overrides?: Partial<RunningIdeationSession>
): RunningIdeationSession {
  return {
    sessionId: "session-abc",
    title: "Test Ideation",
    elapsedSeconds: 120,
    isGenerating: true,
    ...overrides,
  };
}

describe("IdeationSessionCard", () => {
  describe("basic rendering", () => {
    it("renders with correct test id", () => {
      render(<IdeationSessionCard session={createMockSession()} />);
      expect(screen.getByTestId("ideation-card-session-abc")).toBeInTheDocument();
    });

    it("displays session title", () => {
      render(
        <IdeationSessionCard session={createMockSession({ title: "Architecture Review" })} />
      );
      expect(screen.getByText("Architecture Review")).toBeInTheDocument();
    });

    it("displays Ideation badge", () => {
      render(<IdeationSessionCard session={createMockSession()} />);
      expect(screen.getByText("Ideation")).toBeInTheDocument();
    });

    it("title is rendered as a span element (not a button)", () => {
      render(
        <IdeationSessionCard session={createMockSession({ title: "My Session" })} />
      );
      const titleEl = screen.getByText("My Session");
      expect(titleEl.tagName).toBe("SPAN");
    });
  });

  describe("click behavior", () => {
    it("clicking outer div fires onClick", () => {
      const onClick = vi.fn();
      render(<IdeationSessionCard session={createMockSession()} onClick={onClick} />);

      fireEvent.click(screen.getByTestId("ideation-card-session-abc"));

      expect(onClick).toHaveBeenCalledOnce();
    });

    it("pressing Enter on outer div fires onClick", () => {
      const onClick = vi.fn();
      render(<IdeationSessionCard session={createMockSession()} onClick={onClick} />);

      fireEvent.keyDown(screen.getByTestId("ideation-card-session-abc"), { key: "Enter" });

      expect(onClick).toHaveBeenCalledOnce();
    });

    it("pressing Space on outer div fires onClick", () => {
      const onClick = vi.fn();
      render(<IdeationSessionCard session={createMockSession()} onClick={onClick} />);

      fireEvent.keyDown(screen.getByTestId("ideation-card-session-abc"), { key: " " });

      expect(onClick).toHaveBeenCalledOnce();
    });

    it("pressing other keys does NOT fire onClick", () => {
      const onClick = vi.fn();
      render(<IdeationSessionCard session={createMockSession()} onClick={onClick} />);

      fireEvent.keyDown(screen.getByTestId("ideation-card-session-abc"), { key: "Tab" });
      fireEvent.keyDown(screen.getByTestId("ideation-card-session-abc"), { key: "Escape" });

      expect(onClick).not.toHaveBeenCalled();
    });
  });

  describe("accessibility attributes", () => {
    it("has role=button and tabIndex=0 when onClick provided", () => {
      render(
        <IdeationSessionCard session={createMockSession()} onClick={vi.fn()} />
      );
      const card = screen.getByTestId("ideation-card-session-abc");
      expect(card).toHaveAttribute("role", "button");
      expect(card).toHaveAttribute("tabindex", "0");
    });

    it("has no role or tabIndex when onClick is undefined", () => {
      render(<IdeationSessionCard session={createMockSession()} />);
      const card = screen.getByTestId("ideation-card-session-abc");
      expect(card).not.toHaveAttribute("role");
      expect(card).not.toHaveAttribute("tabindex");
    });
  });

  describe("generating state", () => {
    it("shows spinner when isGenerating=true", () => {
      render(
        <IdeationSessionCard session={createMockSession({ isGenerating: true })} />
      );
      // Loader2 spinner has animate-spin class
      const card = screen.getByTestId("ideation-card-session-abc");
      const spinner = card.querySelector(".animate-spin");
      expect(spinner).toBeInTheDocument();
    });

    it("shows pause icon when isGenerating=false", () => {
      render(
        <IdeationSessionCard session={createMockSession({ isGenerating: false })} />
      );
      // Spinner should not be present when not generating
      const card = screen.getByTestId("ideation-card-session-abc");
      const spinner = card.querySelector(".animate-spin");
      expect(spinner).not.toBeInTheDocument();
    });
  });

});
