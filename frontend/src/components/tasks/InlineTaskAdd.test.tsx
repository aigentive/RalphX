/**
 * Tests for InlineTaskAdd component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { InlineTaskAdd } from "./InlineTaskAdd";
import { useUiStore } from "@/stores/uiStore";
import * as tauri from "@/lib/tauri";
import type { Task } from "@/types/task";

// Mock Tauri API
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      create: vi.fn(),
    },
  },
}));

// Mock sonner toast
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

const mockCreateTask = vi.fn();

describe("InlineTaskAdd", () => {
  let queryClient: QueryClient;
  const mockOnCreated = vi.fn();
  const mockOpenTaskCreation = vi.fn();

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
        mutations: { retry: false },
      },
    });

    // Reset mocks
    mockCreateTask.mockReset();
    mockOnCreated.mockReset();
    mockOpenTaskCreation.mockReset();

    // Setup API mock
    (tauri.api.tasks.create as ReturnType<typeof vi.fn>).mockImplementation(mockCreateTask);

    // Setup store mock
    useUiStore.setState({ openTaskCreation: mockOpenTaskCreation });
  });

  const renderComponent = (props = {}) => {
    return render(
      <QueryClientProvider client={queryClient}>
        <InlineTaskAdd projectId="project-1" columnId="backlog" onCreated={mockOnCreated} {...props} />
      </QueryClientProvider>
    );
  };

  describe("Collapsed State", () => {
    it("renders ghost card with dashed border", () => {
      renderComponent();

      const ghostCard = screen.getByTestId("inline-task-add-collapsed");
      expect(ghostCard).toBeInTheDocument();
      expect(ghostCard).toHaveTextContent("Add task");
    });

    it("expands when clicked", () => {
      renderComponent();

      const ghostCard = screen.getByTestId("inline-task-add-collapsed");
      fireEvent.click(ghostCard);

      expect(screen.getByTestId("inline-task-add-expanded")).toBeInTheDocument();
      expect(screen.getByTestId("inline-task-add-input")).toBeInTheDocument();
    });

    it("auto-expands from a collapsed column request", () => {
      const onAutoExpandConsumed = vi.fn();
      renderComponent({ autoExpandKey: 1, onAutoExpandConsumed });

      expect(screen.getByTestId("inline-task-add-expanded")).toBeInTheDocument();
      expect(screen.getByTestId("inline-task-add-input")).toBeInTheDocument();
      expect(onAutoExpandConsumed).toHaveBeenCalledTimes(1);
    });

    it("uses collapsed hover styling class", () => {
      renderComponent();

      const ghostCard = screen.getByTestId("inline-task-add-collapsed");
      expect(ghostCard).toHaveClass("inline-task-add-collapsed");
    });
  });

  describe("Expanded State", () => {
    it("auto-focuses input when expanded", async () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));

      await waitFor(() => {
        const input = screen.getByTestId("inline-task-add-input");
        expect(input).toHaveFocus();
      });
    });

    it("updates title as user types", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input") as HTMLInputElement;

      fireEvent.change(input, { target: { value: "New task title" } });
      expect(input.value).toBe("New task title");
    });

    it("creates task on Enter key", async () => {
      const mockTask: Task = {
        id: "task-1",
        projectId: "project-1",
        title: "New task",
        category: "feature",
        priority: 3,
        internalStatus: "backlog",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        archivedAt: null,
      };

      mockCreateTask.mockResolvedValueOnce(mockTask);

      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "New task" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => {
        expect(mockCreateTask).toHaveBeenCalledWith({
          projectId: "project-1",
          title: "New task",
          category: "feature",
          priority: 3,
        });
        expect(mockOnCreated).toHaveBeenCalledWith(mockTask);
      });

      // Should collapse after creation
      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
      expect(screen.getByTestId("inline-task-add-collapsed")).toBeInTheDocument();
    });

    it("creates task with execution plan scope when provided", async () => {
      const mockTask: Task = {
        id: "task-1",
        projectId: "project-1",
        title: "Plan task",
        category: "feature",
        priority: 3,
        internalStatus: "backlog",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        archivedAt: null,
      };

      mockCreateTask.mockResolvedValueOnce(mockTask);

      renderComponent({
        ideationSessionId: "session-1",
        executionPlanId: "exec-plan-1",
      });

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "Plan task" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => {
        expect(mockCreateTask).toHaveBeenCalledWith(
          expect.objectContaining({
            projectId: "project-1",
            title: "Plan task",
            executionPlanId: "exec-plan-1",
          })
        );
      });
      expect(mockCreateTask.mock.calls[0]?.[0]).not.toHaveProperty("ideationSessionId");
    });

    it("trims whitespace from title before creating", async () => {
      const mockTask: Task = {
        id: "task-1",
        projectId: "project-1",
        title: "Trimmed task",
        category: "feature",
        priority: 3,
        internalStatus: "backlog",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        archivedAt: null,
      };

      mockCreateTask.mockResolvedValueOnce(mockTask);

      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "  Trimmed task  " } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => {
        expect(mockCreateTask).toHaveBeenCalledWith(
          expect.objectContaining({
            title: "Trimmed task",
          })
        );
      });
    });

    it("collapses without creating if title is empty", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.keyDown(input, { key: "Enter" });

      expect(mockCreateTask).not.toHaveBeenCalled();
      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
    });

    it("collapses without creating if title is only whitespace", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "   " } });
      fireEvent.keyDown(input, { key: "Enter" });

      expect(mockCreateTask).not.toHaveBeenCalled();
      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
    });

    it("collapses on Escape key", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      expect(screen.getByTestId("inline-task-add-expanded")).toBeInTheDocument();

      const input = screen.getByTestId("inline-task-add-input");
      fireEvent.change(input, { target: { value: "Some text" } });
      fireEvent.keyDown(input, { key: "Escape" });

      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
      expect(screen.getByTestId("inline-task-add-collapsed")).toBeInTheDocument();
      expect(mockCreateTask).not.toHaveBeenCalled();
    });

    it("opens task creation overlay with pre-filled data when More options clicked", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "My task" } });
      fireEvent.click(screen.getByTestId("inline-task-add-more-options"));

      expect(mockOpenTaskCreation).toHaveBeenCalledWith("project-1", "My task", {});

      // Should collapse after opening overlay
      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
    });

    it("opens task creation overlay with execution plan scope", () => {
      renderComponent({ executionPlanId: "exec-plan-1" });

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "Scoped task" } });
      fireEvent.click(screen.getByTestId("inline-task-add-more-options"));

      expect(mockOpenTaskCreation).toHaveBeenCalledWith("project-1", "Scoped task", {
        executionPlanId: "exec-plan-1",
      });

      // Should collapse after opening overlay
      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
    });

    it("collapses when Cancel clicked", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      expect(screen.getByTestId("inline-task-add-expanded")).toBeInTheDocument();

      fireEvent.click(screen.getByTestId("inline-task-add-cancel"));

      expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
      expect(screen.getByTestId("inline-task-add-collapsed")).toBeInTheDocument();
    });

    it("clears title when collapsed", () => {
      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input") as HTMLInputElement;

      fireEvent.change(input, { target: { value: "Some text" } });
      expect(input.value).toBe("Some text");

      fireEvent.click(screen.getByTestId("inline-task-add-cancel"));
      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));

      const newInput = screen.getByTestId("inline-task-add-input") as HTMLInputElement;
      expect(newInput.value).toBe("");
    });

    it("disables controls while creating", async () => {
      // Mock a slow creation
      let resolveCreate: (value: Task) => void;
      const createPromise = new Promise<Task>((resolve) => {
        resolveCreate = resolve;
      });
      mockCreateTask.mockReturnValue(createPromise);

      renderComponent();

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "New task" } });
      fireEvent.keyDown(input, { key: "Enter" });

      // Wait for mutation to be in pending state
      await waitFor(() => {
        expect(input).toBeDisabled();
      });

      expect(screen.getByTestId("inline-task-add-more-options")).toBeDisabled();
      expect(screen.getByTestId("inline-task-add-cancel")).toBeDisabled();

      // Resolve the promise
      resolveCreate!({
        id: "task-1",
        projectId: "project-1",
        title: "New task",
        category: "feature",
        priority: 3,
        internalStatus: "backlog",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        archivedAt: null,
      });

      await waitFor(() => {
        expect(screen.queryByTestId("inline-task-add-expanded")).not.toBeInTheDocument();
      });
    });
  });

  describe("Column Integration", () => {
    it("creates task with correct projectId regardless of columnId", async () => {
      const mockTask: Task = {
        id: "task-1",
        projectId: "project-1",
        title: "Draft task",
        category: "feature",
        priority: 3,
        internalStatus: "backlog",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        archivedAt: null,
      };

      mockCreateTask.mockResolvedValueOnce(mockTask);

      renderComponent({ columnId: "draft", projectId: "project-1" });

      fireEvent.click(screen.getByTestId("inline-task-add-collapsed"));
      const input = screen.getByTestId("inline-task-add-input");

      fireEvent.change(input, { target: { value: "Draft task" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => {
        expect(mockCreateTask).toHaveBeenCalledWith(
          expect.objectContaining({
            projectId: "project-1",
            title: "Draft task",
          })
        );
      });
    });
  });
});
