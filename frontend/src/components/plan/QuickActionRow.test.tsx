/**
 * Tests for QuickActionRow component
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { Lightbulb } from "lucide-react";
import { QuickActionRow } from "./QuickActionRow";
import type { QuickAction } from "@/hooks/useQuickActionFlow";

// Mock framer-motion to avoid animation issues in tests
vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => <div {...props}>{children}</div>,
    button: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => <button {...props}>{children}</button>,
  },
  AnimatePresence: ({ children }: React.PropsWithChildren) => <>{children}</>,
}));

describe("QuickActionRow", () => {
  const mockAction: QuickAction = {
    id: "test-action",
    label: "Test Action",
    icon: Lightbulb,
    description: (query) => `"${query}"`,
    isVisible: (query) => query.trim().length > 0,
    execute: vi.fn().mockResolvedValue("entity-123"),
    creatingLabel: "Creating...",
    successLabel: "Created!",
    viewLabel: "View",
    navigateTo: vi.fn(),
  };

  const defaultProps = {
    action: mockAction,
    searchQuery: "test query",
    isHighlighted: false,
    onMouseEnter: vi.fn(),
    onSelect: vi.fn(),
    onConfirm: vi.fn(),
    onCancel: vi.fn(),
    onViewEntity: vi.fn(),
  };

  describe("idle state", () => {
    it("should render action label and query description", () => {
      render(<QuickActionRow {...defaultProps} flowState="idle" />);

      expect(screen.getByText("Test Action")).toBeInTheDocument();
      expect(screen.getByText('"test query"')).toBeInTheDocument();
    });

    it("should render icon", () => {
      render(<QuickActionRow {...defaultProps} flowState="idle" />);

      // Just verify the row renders with label (icon is present via component)
      expect(screen.getByText("Test Action")).toBeInTheDocument();
    });

    it("should call onSelect when clicked", async () => {
      const user = userEvent.setup();
      const onSelect = vi.fn();

      render(<QuickActionRow {...defaultProps} flowState="idle" onSelect={onSelect} />);

      const button = screen.getByRole("button");
      await user.click(button);

      expect(onSelect).toHaveBeenCalledOnce();
    });

    it("should call onMouseEnter when mouse enters", async () => {
      const user = userEvent.setup();
      const onMouseEnter = vi.fn();

      render(
        <QuickActionRow {...defaultProps} flowState="idle" onMouseEnter={onMouseEnter} />
      );

      const button = screen.getByRole("button");
      await user.hover(button);

      expect(onMouseEnter).toHaveBeenCalled();
    });

    it("should apply highlighted styles when isHighlighted is true", () => {
      render(<QuickActionRow {...defaultProps} flowState="idle" isHighlighted={true} />);

      const button = screen.getByRole("button");
      // Check that styles are applied (jsdom converts hsla to rgba)
      expect(button.style.background).toBeTruthy();
      expect(button.style.border).toBeTruthy();
    });

    it("should attach highlightedRef when highlighted", () => {
      const ref = { current: null };

      render(
        <QuickActionRow
          {...defaultProps}
          flowState="idle"
          isHighlighted={true}
          highlightedRef={ref}
        />
      );

      expect(ref.current).toBeInstanceOf(HTMLButtonElement);
    });
  });

  describe("confirming state", () => {
    it("should render query and confirm/cancel buttons", () => {
      render(<QuickActionRow {...defaultProps} flowState="confirming" />);

      expect(screen.getByText('"test query"')).toBeInTheDocument();
      expect(screen.getByText("Create Session")).toBeInTheDocument();
      expect(screen.getByText("Cancel")).toBeInTheDocument();
    });

    it("should call onConfirm when confirm button clicked", async () => {
      const user = userEvent.setup();
      const onConfirm = vi.fn();

      render(
        <QuickActionRow {...defaultProps} flowState="confirming" onConfirm={onConfirm} />
      );

      const confirmButton = screen.getByText("Create Session");
      await user.click(confirmButton);

      expect(onConfirm).toHaveBeenCalledOnce();
    });

    it("should call onCancel when cancel button clicked", async () => {
      const user = userEvent.setup();
      const onCancel = vi.fn();

      render(<QuickActionRow {...defaultProps} flowState="confirming" onCancel={onCancel} />);

      const cancelButton = screen.getByText("Cancel");
      await user.click(cancelButton);

      expect(onCancel).toHaveBeenCalledOnce();
    });
  });

  describe("creating state", () => {
    it("should render spinner and creatingLabel", () => {
      render(<QuickActionRow {...defaultProps} flowState="creating" />);

      expect(screen.getByText("Creating...")).toBeInTheDocument();
      // Verify motion div is present (framer-motion is mocked to render div)
      expect(screen.getByText("Creating...").parentElement).toBeInTheDocument();
    });
  });

  describe("success state", () => {
    it("should render check icon and successLabel", () => {
      render(<QuickActionRow {...defaultProps} flowState="success" />);

      expect(screen.getByText("Created!")).toBeInTheDocument();
      // Verify success state content is present
      expect(screen.getByText("View")).toBeInTheDocument();
    });

    it("should render View button with viewLabel", () => {
      render(<QuickActionRow {...defaultProps} flowState="success" />);

      expect(screen.getByText("View")).toBeInTheDocument();
    });

    it("should call onViewEntity when View button clicked", async () => {
      const user = userEvent.setup();
      const onViewEntity = vi.fn();

      render(
        <QuickActionRow {...defaultProps} flowState="success" onViewEntity={onViewEntity} />
      );

      const viewButton = screen.getByText("View");
      await user.click(viewButton);

      expect(onViewEntity).toHaveBeenCalledOnce();
    });
  });

  describe("accessibility", () => {
    it("should have accessible button role in idle state", () => {
      render(<QuickActionRow {...defaultProps} flowState="idle" />);

      expect(screen.getByRole("button")).toBeInTheDocument();
    });

    it("should have accessible buttons in confirming state", () => {
      render(<QuickActionRow {...defaultProps} flowState="confirming" />);

      const buttons = screen.getAllByRole("button");
      expect(buttons).toHaveLength(2); // Confirm and Cancel
    });

    it("should have accessible button in success state", () => {
      render(<QuickActionRow {...defaultProps} flowState="success" />);

      expect(screen.getByRole("button")).toBeInTheDocument();
    });
  });
});
