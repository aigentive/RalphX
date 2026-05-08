import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TaskEditForm } from "./TaskEditForm";
import type { Task } from "@/types/task";

// Helper to render with QueryClientProvider
const renderWithProvider = (ui: React.ReactElement) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>
  );
};

describe("TaskEditForm", () => {
  const mockTask: Task = {
    id: "task-1",
    projectId: "proj-1",
    category: "feature",
    title: "Original Title",
    description: "Original description",
    priority: 1,
    internalStatus: "backlog",
    needsReviewPoint: false,
    sourceProposalId: null,
    planArtifactId: null,
    createdAt: "2024-01-01T00:00:00Z",
    updatedAt: "2024-01-01T00:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
  };

  const mockOnSave = vi.fn();
  const mockOnCancel = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders with pre-populated task data", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    expect(screen.getByDisplayValue("Original Title")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Original description")).toBeInTheDocument();

    // For select elements, check the selected option
    const categorySelect = screen.getByLabelText("Category") as HTMLSelectElement;
    expect(categorySelect.value).toBe("feature");

    const prioritySelect = screen.getByLabelText("Priority") as HTMLSelectElement;
    expect(prioritySelect.value).toBe("1");
  });

  it("calls onSave with changed fields when form is submitted", async () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const titleInput = screen.getByLabelText("Title");
    const categorySelect = screen.getByLabelText("Category");

    fireEvent.change(titleInput, { target: { value: "New Title" } });
    fireEvent.change(categorySelect, { target: { value: "bug" } });

    fireEvent.submit(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        title: "New Title",
        category: "bug",
      });
    });
  });

  it("calls onCancel when no fields are changed", async () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    fireEvent.submit(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockOnCancel).toHaveBeenCalled();
      expect(mockOnSave).not.toHaveBeenCalled();
    });
  });

  it("trims whitespace from title and description", async () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const titleInput = screen.getByLabelText("Title");
    const descInput = screen.getByLabelText("Description");

    fireEvent.change(titleInput, { target: { value: "  New Title  " } });
    fireEvent.change(descInput, { target: { value: "  New Desc  " } });

    fireEvent.submit(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        title: "New Title",
        description: "New Desc",
      });
    });
  });

  it("converts empty description to null", async () => {
    const taskWithDesc: Task = { ...mockTask, description: "Has description" };
    renderWithProvider(
      <TaskEditForm
        task={taskWithDesc}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const descInput = screen.getByLabelText("Description");
    fireEvent.change(descInput, { target: { value: "   " } }); // Only whitespace

    fireEvent.submit(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        description: null,
      });
    });
  });

  it("disables form controls when isSaving is true", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={true}
      />
    );

    expect(screen.getByLabelText("Title")).toBeDisabled();
    expect(screen.getByLabelText("Category")).toBeDisabled();
    expect(screen.getByLabelText("Description")).toBeDisabled();
    expect(screen.getByLabelText("Priority")).toBeDisabled();
    expect(screen.getByRole("button", { name: /cancel/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /saving/i })).toBeDisabled();
  });

  it("shows loading spinner when isSaving is true", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={true}
      />
    );

    expect(screen.getByText("Saving...")).toBeInTheDocument();
  });

  it("calls onCancel when Cancel button is clicked", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));

    expect(mockOnCancel).toHaveBeenCalled();
  });

  it("disables Save button when no changes are made", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const saveButton = screen.getByRole("button", { name: /save changes/i });
    expect(saveButton).toBeDisabled();
  });

  it("enables Save button when changes are made", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const titleInput = screen.getByLabelText("Title");
    fireEvent.change(titleInput, { target: { value: "Changed Title" } });

    const saveButton = screen.getByRole("button", { name: /save changes/i });
    expect(saveButton).not.toBeDisabled();
  });

  it("updates all editable fields", async () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    fireEvent.change(screen.getByLabelText("Title"), {
      target: { value: "New Title" },
    });
    fireEvent.change(screen.getByLabelText("Category"), {
      target: { value: "docs" },
    });
    fireEvent.change(screen.getByLabelText("Description"), {
      target: { value: "New description" },
    });
    fireEvent.change(screen.getByLabelText("Priority"), {
      target: { value: "3" },
    });

    fireEvent.submit(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        title: "New Title",
        category: "docs",
        description: "New description",
        priority: 3,
      });
    });
  });

  it("does not submit when title is empty", () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const titleInput = screen.getByLabelText("Title");
    fireEvent.change(titleInput, { target: { value: "   " } }); // Only whitespace

    const saveButton = screen.getByRole("button", { name: /save changes/i });
    expect(saveButton).toBeDisabled();
  });

  it("handles task with null description", () => {
    const taskWithoutDesc: Task = { ...mockTask, description: null };
    renderWithProvider(
      <TaskEditForm
        task={taskWithoutDesc}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const descInput = screen.getByLabelText("Description") as HTMLTextAreaElement;
    expect(descInput.value).toBe("");
  });

  it("handles priority changes correctly", async () => {
    renderWithProvider(
      <TaskEditForm
        task={mockTask}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />
    );

    const prioritySelect = screen.getByLabelText("Priority");
    fireEvent.change(prioritySelect, { target: { value: "0" } });

    fireEvent.submit(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith({
        priority: 0,
      });
    });
  });

  it("shows the 'Cannot edit while executing' label when task is in an active status", () => {
    renderWithProvider(
      <TaskEditForm
        task={{ ...mockTask, internalStatus: "executing" }}
        onSave={mockOnSave}
        onCancel={mockOnCancel}
        isSaving={false}
      />,
    );
    expect(screen.getByText("Cannot edit while executing")).toBeInTheDocument();
  });
});
