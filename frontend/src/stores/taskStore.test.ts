import { describe, it, expect, beforeEach } from "vitest";
import { useTaskStore, selectTasksByStatus } from "./taskStore";
import type { Task } from "@/types/task";

// Helper to create test tasks
const createTestTask = (overrides: Partial<Task> = {}): Task => ({
  id: `task-${Math.random().toString(36).slice(2)}`,
  projectId: "project-1",
  category: "feature",
  title: "Test Task",
  description: null,
  priority: 0,
  internalStatus: "backlog",
  createdAt: "2026-01-24T12:00:00Z",
  updatedAt: "2026-01-24T12:00:00Z",
  startedAt: null,
  completedAt: null,
  ...overrides,
});

describe("taskStore", () => {
  beforeEach(() => {
    // Reset store to initial state before each test
    useTaskStore.setState({
      tasks: {},
    });
  });

  describe("setTasks", () => {
    it("converts array to Record keyed by id", () => {
      const tasks = [
        createTestTask({ id: "task-1", title: "Task 1" }),
        createTestTask({ id: "task-2", title: "Task 2" }),
        createTestTask({ id: "task-3", title: "Task 3" }),
      ];

      useTaskStore.getState().setTasks(tasks);

      const state = useTaskStore.getState();
      expect(Object.keys(state.tasks)).toHaveLength(3);
      expect(state.tasks["task-1"]?.title).toBe("Task 1");
      expect(state.tasks["task-2"]?.title).toBe("Task 2");
      expect(state.tasks["task-3"]?.title).toBe("Task 3");
    });

    it("replaces existing tasks", () => {
      useTaskStore.setState({
        tasks: {
          "old-task": createTestTask({ id: "old-task", title: "Old Task" }),
        },
      });

      const newTasks = [createTestTask({ id: "new-task", title: "New Task" })];
      useTaskStore.getState().setTasks(newTasks);

      const state = useTaskStore.getState();
      expect(state.tasks["old-task"]).toBeUndefined();
      expect(state.tasks["new-task"]?.title).toBe("New Task");
    });

    it("handles empty array", () => {
      useTaskStore.getState().setTasks([]);

      const state = useTaskStore.getState();
      expect(Object.keys(state.tasks)).toHaveLength(0);
    });
  });

  describe("updateTask", () => {
    it("modifies existing task", () => {
      const task = createTestTask({ id: "task-1", title: "Original Title" });
      useTaskStore.setState({ tasks: { "task-1": task } });

      useTaskStore.getState().updateTask("task-1", { title: "Updated Title" });

      const state = useTaskStore.getState();
      expect(state.tasks["task-1"]?.title).toBe("Updated Title");
    });

    it("updates multiple fields", () => {
      const task = createTestTask({
        id: "task-1",
        title: "Original",
        priority: 0,
        internalStatus: "backlog",
      });
      useTaskStore.setState({ tasks: { "task-1": task } });

      useTaskStore.getState().updateTask("task-1", {
        title: "Updated",
        priority: 5,
        internalStatus: "ready",
      });

      const state = useTaskStore.getState();
      const updatedTask = state.tasks["task-1"];
      expect(updatedTask?.title).toBe("Updated");
      expect(updatedTask?.priority).toBe(5);
      expect(updatedTask?.internalStatus).toBe("ready");
    });

    it("does nothing if task not found", () => {
      const task = createTestTask({ id: "task-1" });
      useTaskStore.setState({ tasks: { "task-1": task } });

      useTaskStore.getState().updateTask("nonexistent", { title: "Updated" });

      const state = useTaskStore.getState();
      expect(Object.keys(state.tasks)).toHaveLength(1);
      expect(state.tasks["task-1"]?.title).toBe("Test Task");
    });

    it("preserves other task fields", () => {
      const task = createTestTask({
        id: "task-1",
        title: "Original",
        description: "A description",
        priority: 3,
      });
      useTaskStore.setState({ tasks: { "task-1": task } });

      useTaskStore.getState().updateTask("task-1", { title: "Updated" });

      const state = useTaskStore.getState();
      const updatedTask = state.tasks["task-1"];
      expect(updatedTask?.title).toBe("Updated");
      expect(updatedTask?.description).toBe("A description");
      expect(updatedTask?.priority).toBe(3);
    });
  });

  describe("addTask", () => {
    it("adds a new task to the store", () => {
      const task = createTestTask({ id: "task-1" });

      useTaskStore.getState().addTask(task);

      const state = useTaskStore.getState();
      expect(state.tasks["task-1"]).toBeDefined();
      expect(state.tasks["task-1"]?.title).toBe("Test Task");
    });

    it("overwrites task with same id", () => {
      const task1 = createTestTask({ id: "task-1", title: "First" });
      const task2 = createTestTask({ id: "task-1", title: "Second" });

      useTaskStore.getState().addTask(task1);
      useTaskStore.getState().addTask(task2);

      const state = useTaskStore.getState();
      expect(state.tasks["task-1"]?.title).toBe("Second");
    });
  });

  describe("removeTask", () => {
    it("removes a task from the store", () => {
      const task = createTestTask({ id: "task-1" });
      useTaskStore.setState({ tasks: { "task-1": task } });

      useTaskStore.getState().removeTask("task-1");

      const state = useTaskStore.getState();
      expect(state.tasks["task-1"]).toBeUndefined();
    });

  });
});

describe("selectors", () => {
  beforeEach(() => {
    useTaskStore.setState({
      tasks: {},
    });
  });

  describe("selectTasksByStatus", () => {
    it("returns tasks with matching status", () => {
      const tasks = [
        createTestTask({ id: "task-1", internalStatus: "backlog" }),
        createTestTask({ id: "task-2", internalStatus: "ready" }),
        createTestTask({ id: "task-3", internalStatus: "backlog" }),
      ];
      useTaskStore.getState().setTasks(tasks);

      const selector = selectTasksByStatus("backlog");
      const result = selector(useTaskStore.getState());

      expect(result).toHaveLength(2);
      expect(result.map((t) => t.id).sort()).toEqual(["task-1", "task-3"]);
    });

    it("returns empty array when no tasks match", () => {
      const tasks = [
        createTestTask({ id: "task-1", internalStatus: "backlog" }),
      ];
      useTaskStore.getState().setTasks(tasks);

      const selector = selectTasksByStatus("approved");
      const result = selector(useTaskStore.getState());

      expect(result).toHaveLength(0);
    });

    it("returns empty array when store is empty", () => {
      const selector = selectTasksByStatus("backlog");
      const result = selector(useTaskStore.getState());

      expect(result).toHaveLength(0);
    });
  });

});
