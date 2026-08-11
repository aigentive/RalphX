/**
 * QuestionInputBanner component tests
 * Tests rendering, chip interaction, dismiss handlers, and dimming logic
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QuestionInputBanner } from "./QuestionInputBanner";
import { computeQuestionHeight } from "./QuestionInputBanner.utils";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";

// ============================================================================
// Test Data
// ============================================================================

const singleSelectQuestion: AskUserQuestionPayload = {
  requestId: "req-1",
  question: "Which framework should we use?",
  header: "Architecture Decision",
  options: [
    { label: "React" },
    { label: "Vue", value: "vue" },
    { label: "Svelte", value: "svelte" },
  ],
  multiSelect: false,
};

const multiSelectQuestion: AskUserQuestionPayload = {
  requestId: "req-2",
  question: "Select the features to include:",
  header: "Feature Selection",
  options: [
    { label: "Auth", value: "auth" },
    { label: "Logging", value: "logging" },
    { label: "Caching", value: "caching" },
  ],
  multiSelect: true,
};

const noHeaderQuestion: AskUserQuestionPayload = {
  requestId: "req-3",
  question: "Continue with this approach?",
  options: [
    { label: "Yes", value: "yes" },
    { label: "No", value: "no" },
  ],
  multiSelect: false,
};

const defaultProps = {
  question: singleSelectQuestion,
  selectedIndices: new Set<number>(),
  onChipClick: vi.fn(),
  onDismiss: vi.fn(),
};

// ============================================================================
// Tests
// ============================================================================

describe("QuestionInputBanner", () => {
  describe("Active Question State", () => {
    it("renders the question header", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      expect(screen.getByText("Architecture Decision")).toBeInTheDocument();
    });

    it("renders fallback header when header is null", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          question={noHeaderQuestion}
        />
      );

      expect(screen.getByText("Question from agent")).toBeInTheDocument();
    });

    it("renders the question text", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      expect(
        screen.getByText("Which framework should we use?")
      ).toBeInTheDocument();
    });

    it("renders all option chips", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      expect(screen.getByText("React")).toBeInTheDocument();
      expect(screen.getByText("Vue")).toBeInTheDocument();
      expect(screen.getByText("Svelte")).toBeInTheDocument();
    });

    it("renders numbered chips starting at 1", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      expect(screen.getByText("1")).toBeInTheDocument();
      expect(screen.getByText("2")).toBeInTheDocument();
      expect(screen.getByText("3")).toBeInTheDocument();
    });

    it("renders the dismiss button", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      expect(
        screen.getByRole("button", { name: "Dismiss question" })
      ).toBeInTheDocument();
    });

    it("renders the data-testid on the outer wrapper", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      expect(screen.getByTestId("question-input-banner")).toBeInTheDocument();
    });

    it("renders an Approve Plan action without replacing answer controls", async () => {
      const user = userEvent.setup();
      const onApprovePlan = vi.fn();
      const onChipClick = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          onChipClick={onChipClick}
          planApprovalAction={{
            label: "Approve Plan",
            onClick: onApprovePlan,
          }}
        />
      );

      await user.click(screen.getByRole("button", { name: "Approve Plan" }));
      await user.click(screen.getByRole("button", { name: /1\s*React/ }));

      expect(onApprovePlan).toHaveBeenCalledTimes(1);
      expect(onChipClick).toHaveBeenCalledWith(0);
    });

    it("uses a custom pending label for inline question actions", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          planApprovalAction={{
            label: "Apply proposal",
            pendingLabel: "Applying...",
            onClick: vi.fn(),
            isPending: true,
          }}
        />
      );

      expect(screen.getByRole("button", { name: "Applying..." })).toBeDisabled();
    });

    it("renders batch progress and skip action when skipping is allowed", async () => {
      const user = userEvent.setup();
      const onSkip = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={{
            ...singleSelectQuestion,
            batchIndex: 2,
            batchTotal: 3,
            allowSkip: true,
          }}
          onSkip={onSkip}
        />
      );

      expect(screen.getByText("2 of 3")).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Skip" }));
      expect(onSkip).toHaveBeenCalledTimes(1);
    });

    it("does not render skip action when skipping is disabled", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          question={{
            ...singleSelectQuestion,
            allowSkip: false,
          }}
          onSkip={vi.fn()}
        />
      );

      expect(screen.queryByRole("button", { name: "Skip" })).not.toBeInTheDocument();
    });
  });

  describe("Answered/Collapsed State", () => {
    it("renders answered state when answeredValue is set", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          answeredValue="React"
        />
      );

      expect(
        screen.getByTestId("question-input-banner-answered")
      ).toBeInTheDocument();
    });

    it("shows the answered value text", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          answeredValue="React"
        />
      );

      expect(screen.getByText("React")).toBeInTheDocument();
      expect(screen.getByText("Answered:")).toBeInTheDocument();
    });

    it("keeps the plan approval action visible in answered state", async () => {
      const user = userEvent.setup();
      const onApprovePlan = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={null}
          answeredValue="React"
          planApprovalAction={{
            label: "Approve Plan",
            onClick: onApprovePlan,
          }}
        />
      );

      await user.click(screen.getByTestId("question-plan-approval-action"));
      expect(onApprovePlan).toHaveBeenCalledTimes(1);
    });

    it("does not render question text or chips in answered state", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          answeredValue="React"
        />
      );

      expect(
        screen.queryByText("Which framework should we use?")
      ).not.toBeInTheDocument();
      expect(screen.queryByText("Vue")).not.toBeInTheDocument();
    });

    it("renders dismiss button in answered state when onDismissAnswered provided", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          answeredValue="React"
          onDismissAnswered={vi.fn()}
        />
      );

      expect(
        screen.getByRole("button", { name: "Dismiss answered summary" })
      ).toBeInTheDocument();
    });

    it("does not render dismiss button in answered state when onDismissAnswered not provided", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          answeredValue="React"
        />
      );

      expect(
        screen.queryByRole("button", { name: "Dismiss answered summary" })
      ).not.toBeInTheDocument();
    });
  });

  describe("Chip Click Handler", () => {
    it("calls onChipClick with correct index when chip is clicked", async () => {
      const user = userEvent.setup();
      const onChipClick = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          onChipClick={onChipClick}
        />
      );

      await user.click(screen.getByText("Vue"));
      expect(onChipClick).toHaveBeenCalledWith(1);
    });

    it("calls onChipClick with index 0 for first chip", async () => {
      const user = userEvent.setup();
      const onChipClick = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          onChipClick={onChipClick}
        />
      );

      await user.click(screen.getByText("React"));
      expect(onChipClick).toHaveBeenCalledWith(0);
    });

    it("calls onChipClick with last index for last chip", async () => {
      const user = userEvent.setup();
      const onChipClick = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          onChipClick={onChipClick}
        />
      );

      await user.click(screen.getByText("Svelte"));
      expect(onChipClick).toHaveBeenCalledWith(2);
    });
  });

  describe("Dismiss Handlers", () => {
    it("calls onDismiss after animation delay when dismiss button is clicked", async () => {
      const user = userEvent.setup();
      const onDismiss = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          onDismiss={onDismiss}
        />
      );

      await user.click(
        screen.getByRole("button", { name: "Dismiss question" })
      );

      // onDismiss is called after 350ms animation delay via setTimeout
      await vi.waitFor(() => {
        expect(onDismiss).toHaveBeenCalledTimes(1);
      }, { timeout: 500 });
    });

    it("calls onDismissAnswered after animation delay when answered dismiss is clicked", async () => {
      const user = userEvent.setup();
      const onDismissAnswered = vi.fn();

      render(
        <QuestionInputBanner
          {...defaultProps}
          answeredValue="React"
          onDismissAnswered={onDismissAnswered}
        />
      );

      await user.click(
        screen.getByRole("button", { name: "Dismiss answered summary" })
      );

      await vi.waitFor(() => {
        expect(onDismissAnswered).toHaveBeenCalledTimes(1);
      }, { timeout: 500 });
    });
  });

  describe("Single-Select Dimming Logic", () => {
    it("does not dim chips when nothing is selected", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      const buttons = screen.getAllByRole("button").filter(
        (b) => !b.getAttribute("aria-label")
      );

      // All chips should have opacity 1 (not dimmed)
      for (const button of buttons) {
        expect(button.style.opacity).toBe("1");
      }
    });

    it("dims unselected chips when one is selected in single-select mode", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          selectedIndices={new Set([1])}
        />
      );

      const buttons = screen.getAllByRole("button").filter(
        (b) => !b.getAttribute("aria-label")
      );

      // First chip (React, index 0) - dimmed
      expect(buttons[0].style.opacity).toBe("0.45");
      // Second chip (Vue, index 1) - selected, not dimmed
      expect(buttons[1].style.opacity).toBe("1");
      // Third chip (Svelte, index 2) - dimmed
      expect(buttons[2].style.opacity).toBe("0.45");
    });

    it("does not dim chips in multi-select mode even with selections", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          question={multiSelectQuestion}
          selectedIndices={new Set([0])}
        />
      );

      const buttons = screen.getAllByRole("button").filter(
        (b) => !b.getAttribute("aria-label")
      );

      // In multi-select, no chips should be dimmed
      for (const button of buttons) {
        expect(button.style.opacity).toBe("1");
      }
    });
  });

  describe("Multi-Select Checkmarks", () => {
    it("renders checkmark icons in multi-select mode", () => {
      render(
        <QuestionInputBanner
          {...defaultProps}
          question={multiSelectQuestion}
        />
      );

      // Check SVG elements exist (lucide Check icons)
      const banner = screen.getByTestId("question-input-banner");
      const svgs = banner.querySelectorAll("svg");
      // Should have checkmarks for each option (3 options in multiSelectQuestion)
      // plus the dismiss X button
      expect(svgs.length).toBeGreaterThanOrEqual(3);
    });

    it("does not render checkmark icons in single-select mode", () => {
      render(<QuestionInputBanner {...defaultProps} />);

      const banner = screen.getByTestId("question-input-banner");
      // In single-select, only the dismiss X button should have an SVG
      const svgs = banner.querySelectorAll("svg");
      expect(svgs.length).toBe(1); // Only the X dismiss button
    });
  });

  describe("Expand/Collapse Toggle", () => {
    it("does not render expand button when computed height < 280px", () => {
      const smallQuestion: AskUserQuestionPayload = {
        requestId: "req-small",
        question: "Simple?",
        options: [{ label: "Yes" }, { label: "No" }],
        multiSelect: false,
      };

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={smallQuestion}
        />
      );

      // Small question should have computed height < 280, so expand button should not be visible
      expect(
        screen.queryByRole("button", {
          name: /expand question|collapse question/i,
        })
      ).not.toBeInTheDocument();
    });

    it("renders collapse button when computed height >= 280px (starts expanded)", () => {
      const largeQuestion: AskUserQuestionPayload = {
        requestId: "req-large",
        question: "This is a very long question that will take up significant space and cause the computed height to exceed 280 pixels threshold.",
        options: Array.from({ length: 6 }, (_, i) => ({
          label: `Option ${i + 1} with longer text`,
        })),
        multiSelect: false,
      };

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={largeQuestion}
        />
      );

      // Large question should have computed height >= 280, so collapse button should be visible
      // (banner starts expanded by default)
      expect(
        screen.getByRole("button", { name: "Collapse question" })
      ).toBeInTheDocument();
    });

    it("toggles expand state and icon when button is clicked", async () => {
      const user = userEvent.setup();
      const largeQuestion: AskUserQuestionPayload = {
        requestId: "req-large",
        question: "This is a very long question that will take up significant space and cause the computed height to exceed 280 pixels threshold.",
        options: Array.from({ length: 6 }, (_, i) => ({
          label: `Option ${i + 1} with longer text`,
        })),
        multiSelect: false,
      };

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={largeQuestion}
        />
      );

      // Initially should show collapse icon (starts expanded by default)
      expect(
        screen.getByRole("button", { name: "Collapse question" })
      ).toBeInTheDocument();

      // Click collapse button
      await user.click(
        screen.getByRole("button", { name: "Collapse question" })
      );

      // Should now show expand icon
      expect(
        screen.getByRole("button", { name: "Expand question" })
      ).toBeInTheDocument();

      // Click expand button
      await user.click(
        screen.getByRole("button", { name: "Expand question" })
      );

      // Should show collapse icon again
      expect(
        screen.getByRole("button", { name: "Collapse question" })
      ).toBeInTheDocument();
    });

    it("updates container maxHeight to 60vh when expanded (starts expanded)", async () => {
      const user = userEvent.setup();
      const largeQuestion: AskUserQuestionPayload = {
        requestId: "req-large",
        question: "This is a very long question that will take up significant space and cause the computed height to exceed 280 pixels threshold.",
        options: Array.from({ length: 6 }, (_, i) => ({
          label: `Option ${i + 1} with longer text`,
        })),
        multiSelect: false,
      };

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={largeQuestion}
        />
      );

      const container = screen.getByTestId("question-input-banner");

      // Wait for the initial animation (visible state changes via requestAnimationFrame)
      await vi.waitFor(
        () => {
          expect(container.style.maxHeight).toBe("60vh");
        },
        { timeout: 100 }
      );

      // Click collapse
      await user.click(
        screen.getByRole("button", { name: "Collapse question" })
      );

      // After collapse, maxHeight should be reduced to computedHeight
      expect(container.style.maxHeight).not.toBe("60vh");

      // Click expand again
      await user.click(
        screen.getByRole("button", { name: "Expand question" })
      );

      // After expansion, maxHeight should be 60vh again
      expect(container.style.maxHeight).toBe("60vh");
    });

    it("resets expand state when question changes (requestId changes) - resets to expanded", async () => {
      const user = userEvent.setup();
      const largeQuestion1: AskUserQuestionPayload = {
        requestId: "req-1",
        question: "This is a very long question that will take up significant space and cause the computed height to exceed 280 pixels threshold when rendered with multiple options.",
        options: Array.from({ length: 8 }, (_, i) => ({
          label: `Option ${i + 1} with longer text`,
        })),
        multiSelect: false,
      };

      const largeQuestion2: AskUserQuestionPayload = {
        requestId: "req-2",
        question: "Another very long question that will also take up significant space and cause the computed height to exceed 280 pixels threshold when rendered with options.",
        options: Array.from({ length: 8 }, (_, i) => ({
          label: `Option ${i + 1} with longer text`,
        })),
        multiSelect: false,
      };

      const { rerender } = render(
        <QuestionInputBanner
          {...defaultProps}
          question={largeQuestion1}
        />
      );

      // Starts expanded (collapse button visible)
      expect(
        screen.getByRole("button", { name: "Collapse question" })
      ).toBeInTheDocument();

      // Collapse the question
      await user.click(
        screen.getByRole("button", { name: "Collapse question" })
      );

      // Verify it's collapsed (expand button visible)
      expect(
        screen.getByRole("button", { name: "Expand question" })
      ).toBeInTheDocument();

      // Change question
      rerender(
        <QuestionInputBanner
          {...defaultProps}
          question={largeQuestion2}
        />
      );

      // After question change, should be expanded again (default state)
      await vi.waitFor(
        () => {
          expect(
            screen.getByRole("button", { name: "Collapse question" })
          ).toBeInTheDocument();
        },
        { timeout: 500 }
      );
      expect(
        screen.queryByRole("button", { name: "Expand question" })
      ).not.toBeInTheDocument();
    });

    it("body has scrollable maxHeight when expanded (starts expanded by default)", async () => {
      const user = userEvent.setup();
      const largeQuestion: AskUserQuestionPayload = {
        requestId: "req-large",
        question: "This is a very long question that will take up significant space and cause the computed height to exceed 280 pixels threshold.",
        options: Array.from({ length: 6 }, (_, i) => ({
          label: `Option ${i + 1} with longer text`,
        })),
        multiSelect: false,
      };

      render(
        <QuestionInputBanner
          {...defaultProps}
          question={largeQuestion}
        />
      );

      // Get the body div (contains question text and chips)
      const bannerContent = screen.getByTestId("question-input-banner");
      const bodyDivs = bannerContent.querySelectorAll("div");
      // The body is the second main div (after header)
      const bodyDiv = Array.from(bodyDivs).find(
        (div) =>
          div.textContent?.includes("This is a very long question") &&
          div.style.padding === "10px 12px 12px"
      );

      expect(bodyDiv).toBeDefined();
      if (bodyDiv) {
        // Since starts expanded, should already have overflow auto and restricted height
        const initialStyle = window.getComputedStyle(bodyDiv);
        expect(initialStyle.overflowY).toBe("auto");
        expect(initialStyle.maxHeight).toBe("calc(60vh - 40px)");
      }

      // Click collapse
      await user.click(
        screen.getByRole("button", { name: "Collapse question" })
      );

      // After collapse, body should not have scrollable overflow
      if (bodyDiv) {
        const collapsedStyle = window.getComputedStyle(bodyDiv);
        expect(collapsedStyle.overflowY).not.toBe("auto");
      }
    });
  });

  describe("computeQuestionHeight function", () => {
    it("returns height between 120px and 320px bounds", () => {
      const question: AskUserQuestionPayload = {
        requestId: "req-small",
        question: "Simple?",
        options: [{ label: "Yes" }, { label: "No" }],
        multiSelect: false,
      };

      const height = computeQuestionHeight(question);
      expect(height).toBeGreaterThanOrEqual(120);
      expect(height).toBeLessThanOrEqual(320);
    });

    it("returns smaller height for 2 short options (target ~160px)", () => {
      const question: AskUserQuestionPayload = {
        requestId: "req-small",
        question: "Pick one",
        options: [{ label: "Yes" }, { label: "No" }],
        multiSelect: false,
      };

      const height = computeQuestionHeight(question);
      // Should be relatively small - short question, few short options
      expect(height).toBeLessThan(180);
    });

    it("returns medium height for 4 medium options (target ~140-200px)", () => {
      const question: AskUserQuestionPayload = {
        requestId: "req-medium",
        question: "Select the environment for deployment",
        options: [
          { label: "Development" },
          { label: "Staging" },
          { label: "Production" },
          { label: "Testing" },
        ],
        multiSelect: false,
      };

      const height = computeQuestionHeight(question);
      // Should be medium-sized - longer question and more options
      expect(height).toBeGreaterThan(130);
      expect(height).toBeLessThan(280);
    });

    it("returns maximum height for many options (capped at 320px)", () => {
      const question: AskUserQuestionPayload = {
        requestId: "req-large",
        question: "This is a longer question that should wrap across multiple lines when displayed in the component.",
        options: Array.from({ length: 8 }, (_, i) => ({
          label: `Option ${i + 1} with some text`,
        })),
        multiSelect: false,
      };

      const height = computeQuestionHeight(question);
      // Should hit the 320px cap
      expect(height).toBe(320);
    });

    it("accounts for question text length in height estimation", () => {
      const shortQuestion: AskUserQuestionPayload = {
        requestId: "req-1",
        question: "Pick",
        options: [{ label: "A" }, { label: "B" }],
        multiSelect: false,
      };

      const longQuestion: AskUserQuestionPayload = {
        requestId: "req-2",
        question: "This is a much longer question that will wrap across multiple lines when rendered at the default font size and width constraints of the component.",
        options: [{ label: "A" }, { label: "B" }],
        multiSelect: false,
      };

      const shortHeight = computeQuestionHeight(shortQuestion);
      const longHeight = computeQuestionHeight(longQuestion);

      // Longer question should result in taller height
      expect(longHeight).toBeGreaterThan(shortHeight);
    });

    it("accounts for label lengths in chip width estimation", () => {
      const shortLabels: AskUserQuestionPayload = {
        requestId: "req-1",
        question: "Pick one",
        options: [{ label: "Yes" }, { label: "No" }],
        multiSelect: false,
      };

      const longLabels: AskUserQuestionPayload = {
        requestId: "req-2",
        question: "Pick one",
        options: [
          { label: "Implementation" },
          { label: "Investigation" },
          { label: "Documentation" },
        ],
        multiSelect: false,
      };

      const shortHeight = computeQuestionHeight(shortLabels);
      const longHeight = computeQuestionHeight(longLabels);

      // Longer labels should result in more wrapping and taller height
      expect(longHeight).toBeGreaterThan(shortHeight);
    });

    it("accounts for many options causing multiple chip rows", () => {
      const fewOptions: AskUserQuestionPayload = {
        requestId: "req-1",
        question: "Pick",
        options: [{ label: "A" }, { label: "B" }],
        multiSelect: false,
      };

      const manyOptions: AskUserQuestionPayload = {
        requestId: "req-2",
        question: "Pick",
        options: Array.from({ length: 6 }, (_, i) => ({
          label: `Option ${i + 1}`,
        })),
        multiSelect: false,
      };

      const fewHeight = computeQuestionHeight(fewOptions);
      const manyHeight = computeQuestionHeight(manyOptions);

      // More options should generally result in taller height
      expect(manyHeight).toBeGreaterThan(fewHeight);
    });

    it("respects minimum height of 120px", () => {
      // Even with empty or minimal content, should not go below 120px
      const minimalQuestion: AskUserQuestionPayload = {
        requestId: "req-minimal",
        question: "Q?",
        options: [],
        multiSelect: false,
      };

      const height = computeQuestionHeight(minimalQuestion);
      expect(height).toBeGreaterThanOrEqual(120);
    });

    it("respects maximum height of 320px", () => {
      // Even with large content, should not exceed 320px
      const hugeQuestion: AskUserQuestionPayload = {
        requestId: "req-huge",
        question: Array(500).fill("Lorem ipsum dolor sit amet. ").join(""),
        options: Array.from({ length: 20 }, (_, i) => ({
          label: `Very long option label number ${i + 1} with additional text`,
        })),
        multiSelect: false,
      };

      const height = computeQuestionHeight(hugeQuestion);
      expect(height).toBeLessThanOrEqual(320);
    });
  });
});
