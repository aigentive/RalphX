/**
 * Tests for TaskCreationForm component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TaskCreationForm } from "./TaskCreationForm";

// Mock the useTaskMutation hook
const mockMutate = vi.fn();
const mockReset = vi.fn();

vi.mock("@/hooks/useTaskMutation", () => ({
  useTaskMutation: () => ({
    createMutation: {
      mutate: mockMutate,
      isPending: false,
      isError: false,
      error: null,
      reset: mockReset,
    },
  }),
}));

describe("TaskCreationForm", () => {
  const defaultProps = {
    projectId: "project-123",
    onSuccess: vi.fn(),
    onCancel: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("Rendering", () => {
    it("should render the form with all required fields", () => {
      render(<TaskCreationForm {...defaultProps} />);

      expect(screen.getByLabelText(/title/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/category/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/priority/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/description/i)).toBeInTheDocument();
    });

    it("should render submit and cancel buttons", () => {
      render(<TaskCreationForm {...defaultProps} />);

      expect(screen.getByRole("button", { name: /create task/i })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /cancel/i })).toBeInTheDocument();
    });

    it("should pre-fill title when defaultTitle is provided", () => {
      render(<TaskCreationForm {...defaultProps} defaultTitle="Pre-filled Title" />);

      expect(screen.getByLabelText(/title/i)).toHaveValue("Pre-filled Title");
    });
  });

  describe("Form Validation", () => {
    it("should not submit when title is empty", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).not.toHaveBeenCalled();
    });

    it("should submit form with valid data", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Test Task");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-123",
          title: "Test Task",
          category: "feature", // default
          priority: 3, // default (P3 - Medium)
        }),
        expect.anything()
      );
    });

    it("should submit execution plan scope when provided", async () => {
      render(
        <TaskCreationForm
          {...defaultProps}
          ideationSessionId="session-123"
          executionPlanId="exec-plan-123"
        />
      );

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Plan task");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-123",
          title: "Plan task",
          executionPlanId: "exec-plan-123",
        }),
        expect.anything()
      );
      expect(mockMutate.mock.calls[0]?.[0]).not.toHaveProperty("ideationSessionId");
    });
  });

  describe("Category Selection", () => {
    it("should default to feature category", () => {
      render(<TaskCreationForm {...defaultProps} />);

      const categorySelect = screen.getByLabelText(/category/i);
      expect(categorySelect).toHaveValue("feature");
    });

    it("should allow changing category", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const categorySelect = screen.getByLabelText(/category/i);
      await userEvent.selectOptions(categorySelect, "bug");

      expect(categorySelect).toHaveValue("bug");
    });

    it("should submit with selected category", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Bug Fix");

      const categorySelect = screen.getByLabelText(/category/i);
      await userEvent.selectOptions(categorySelect, "bug");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).toHaveBeenCalledWith(
        expect.objectContaining({
          category: "bug",
        }),
        expect.anything()
      );
    });
  });

  describe("Priority Selection", () => {
    it("should default to P3 (Medium) priority", () => {
      render(<TaskCreationForm {...defaultProps} />);

      const prioritySelect = screen.getByLabelText(/priority/i);
      expect(prioritySelect).toHaveValue("3");
    });

    it("should allow changing priority", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const prioritySelect = screen.getByLabelText(/priority/i);
      await userEvent.selectOptions(prioritySelect, "1");

      expect(prioritySelect).toHaveValue("1");
    });

    it("should submit with selected priority", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Critical Task");

      const prioritySelect = screen.getByLabelText(/priority/i);
      await userEvent.selectOptions(prioritySelect, "1");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).toHaveBeenCalledWith(
        expect.objectContaining({
          priority: 1,
        }),
        expect.anything()
      );
    });
  });

  describe("Description Field", () => {
    it("should allow entering description", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const descriptionInput = screen.getByLabelText(/description/i);
      await userEvent.type(descriptionInput, "This is a test description");

      expect(descriptionInput).toHaveValue("This is a test description");
    });

    it("should submit with description", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Task with Description");

      const descriptionInput = screen.getByLabelText(/description/i);
      await userEvent.type(descriptionInput, "Detailed description here");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).toHaveBeenCalledWith(
        expect.objectContaining({
          description: "Detailed description here",
        }),
        expect.anything()
      );
    });

    it("should submit without description when empty", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Task without Description");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      const callArgs = mockMutate.mock.calls[0]?.[0];
      expect(callArgs?.description).toBeUndefined();
    });
  });

  describe("Cancel Button", () => {
    it("should call onCancel when cancel button clicked", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const cancelButton = screen.getByRole("button", { name: /cancel/i });
      await userEvent.click(cancelButton);

      expect(defaultProps.onCancel).toHaveBeenCalled();
    });

    it("should not submit form when cancel clicked", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const cancelButton = screen.getByRole("button", { name: /cancel/i });
      await userEvent.click(cancelButton);

      expect(mockMutate).not.toHaveBeenCalled();
    });
  });

  describe("Form Reset", () => {
    it("should call onSuccess after successful submission", async () => {
      mockMutate.mockImplementation((_, { onSuccess }) => {
        onSuccess?.();
      });

      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Test Task");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(defaultProps.onSuccess).toHaveBeenCalled();
    });
  });

  describe("Accessibility", () => {
    it("should have proper form labels", () => {
      render(<TaskCreationForm {...defaultProps} />);

      // All inputs should be labeled
      expect(screen.getByLabelText(/title/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/category/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/priority/i)).toBeInTheDocument();
      expect(screen.getByLabelText(/description/i)).toBeInTheDocument();
    });
  });

  describe("Steps Editor", () => {
    it("should render steps section", () => {
      render(<TaskCreationForm {...defaultProps} />);

      expect(screen.getByText(/steps \(optional\)/i)).toBeInTheDocument();
      expect(screen.getByPlaceholderText(/add a step/i)).toBeInTheDocument();
    });

    it("should add a step when clicking Add button", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const stepInput = screen.getByPlaceholderText(/add a step/i);
      await userEvent.type(stepInput, "First step");

      const addButton = screen.getByRole("button", { name: /add/i });
      await userEvent.click(addButton);

      expect(screen.getByText("First step")).toBeInTheDocument();
      expect(stepInput).toHaveValue("");
    });

    it("should add a step when pressing Enter", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const stepInput = screen.getByPlaceholderText(/add a step/i);
      await userEvent.type(stepInput, "First step{enter}");

      expect(screen.getByText("First step")).toBeInTheDocument();
    });

    it("should not add empty steps", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const addButton = screen.getByRole("button", { name: /add/i });
      expect(addButton).toBeDisabled();
    });

    it("should remove a step when clicking remove button", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const stepInput = screen.getByPlaceholderText(/add a step/i);
      await userEvent.type(stepInput, "Step to remove{enter}");

      expect(screen.getByText("Step to remove")).toBeInTheDocument();

      const removeButton = screen.getByTitle(/remove step/i);
      await userEvent.click(removeButton);

      expect(screen.queryByText("Step to remove")).not.toBeInTheDocument();
    });

    it("should reorder steps with up/down buttons", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const stepInput = screen.getByPlaceholderText(/add a step/i);
      await userEvent.type(stepInput, "First step{enter}");
      await userEvent.type(stepInput, "Second step{enter}");

      // Find the step items by looking for the numbered items
      const stepItems = screen.getAllByText(/step$/i);
      expect(stepItems).toHaveLength(2);

      // Move second step up
      const moveUpButtons = screen.getAllByTitle(/move up/i);
      await userEvent.click(moveUpButtons[1]!);

      // Check order changed (Second step should now be first)
      const reorderedItems = screen.getAllByText(/step$/i);
      expect(reorderedItems[0]).toHaveTextContent("Second step");
      expect(reorderedItems[1]).toHaveTextContent("First step");
    });

    it("should include steps in submission data", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Task with Steps");

      const stepInput = screen.getByPlaceholderText(/add a step/i);
      await userEvent.type(stepInput, "Step one{enter}");
      await userEvent.type(stepInput, "Step two{enter}");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      expect(mockMutate).toHaveBeenCalledWith(
        expect.objectContaining({
          steps: ["Step one", "Step two"],
        }),
        expect.anything()
      );
    });

    it("should not include steps in submission when none added", async () => {
      render(<TaskCreationForm {...defaultProps} />);

      const titleInput = screen.getByLabelText(/title/i);
      await userEvent.type(titleInput, "Task without Steps");

      const submitButton = screen.getByRole("button", { name: /create task/i });
      await userEvent.click(submitButton);

      const callArgs = mockMutate.mock.calls[0]?.[0];
      expect(callArgs?.steps).toBeUndefined();
    });
  });
});
