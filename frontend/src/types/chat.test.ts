import { describe, expect, it } from "vitest";
import {
  CHAT_CONTEXT_VIEW_VALUES,
  ChatContextViewSchema,
  ChatContextSchema,
  isKanbanContext,
  isIdeationContext,
  isTaskDetailContext,
  isTicketingContext,
  isGranolaContext,
  createKanbanContext,
  createIdeationContext,
  createTaskDetailContext,
  createProjectContext,
} from "./chat";

describe("ChatContextViewSchema", () => {
  it("should have 12 view type values", () => {
    expect(CHAT_CONTEXT_VIEW_VALUES.length).toBe(12);
  });

  it("should parse all valid view types", () => {
    for (const viewType of CHAT_CONTEXT_VIEW_VALUES) {
      expect(ChatContextViewSchema.parse(viewType)).toBe(viewType);
    }
  });

  it("should include expected view types", () => {
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("kanban");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("graph");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("ideation");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("agents");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("automations");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("extensibility");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("activity");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("ticketing");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("github");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("granola");
    expect(CHAT_CONTEXT_VIEW_VALUES).toContain("task_detail");
  });

  it("should reject invalid view type", () => {
    expect(() => ChatContextViewSchema.parse("invalid")).toThrow();
    expect(() => ChatContextViewSchema.parse("Kanban")).toThrow();
  });
});

describe("ChatContextSchema", () => {
  it("should parse kanban context with no selection", () => {
    const context = {
      view: "kanban" as const,
      projectId: "project-123",
    };
    expect(() => ChatContextSchema.parse(context)).not.toThrow();
    const result = ChatContextSchema.parse(context);
    expect(result.view).toBe("kanban");
    expect(result.projectId).toBe("project-123");
    expect(result.selectedTaskId).toBeUndefined();
  });

  it("should parse kanban context with selected task", () => {
    const context = {
      view: "kanban" as const,
      projectId: "project-123",
      selectedTaskId: "task-456",
    };
    expect(() => ChatContextSchema.parse(context)).not.toThrow();
    const result = ChatContextSchema.parse(context);
    expect(result.selectedTaskId).toBe("task-456");
  });

  it("should parse ideation context", () => {
    const context = {
      view: "ideation" as const,
      projectId: "project-123",
      ideationSessionId: "session-789",
    };
    expect(() => ChatContextSchema.parse(context)).not.toThrow();
    const result = ChatContextSchema.parse(context);
    expect(result.ideationSessionId).toBe("session-789");
  });

  it("should parse task_detail context", () => {
    const context = {
      view: "task_detail" as const,
      projectId: "project-123",
      selectedTaskId: "task-456",
    };
    expect(() => ChatContextSchema.parse(context)).not.toThrow();
  });

  it("should parse activity context", () => {
    const context = {
      view: "activity" as const,
      projectId: "project-123",
    };
    expect(() => ChatContextSchema.parse(context)).not.toThrow();
  });

  it("should reject context with empty project id", () => {
    expect(() =>
      ChatContextSchema.parse({
        view: "kanban",
        projectId: "",
      })
    ).toThrow();
  });

  it("should reject context with invalid view", () => {
    expect(() =>
      ChatContextSchema.parse({
        view: "invalid",
        projectId: "project-123",
      })
    ).toThrow();
  });
});

describe("Context helper functions", () => {
  describe("isKanbanContext", () => {
    it("should return true for kanban view", () => {
      expect(isKanbanContext({ view: "kanban", projectId: "p1" })).toBe(true);
    });

    it("should return false for other views", () => {
      expect(isKanbanContext({ view: "ideation", projectId: "p1" })).toBe(false);
      expect(isKanbanContext({ view: "activity", projectId: "p1" })).toBe(false);
    });
  });

  describe("isIdeationContext", () => {
    it("should return true for ideation view", () => {
      expect(isIdeationContext({ view: "ideation", projectId: "p1" })).toBe(true);
    });

    it("should return false for other views", () => {
      expect(isIdeationContext({ view: "kanban", projectId: "p1" })).toBe(false);
    });
  });

  describe("isTaskDetailContext", () => {
    it("should return true for task_detail view", () => {
      expect(isTaskDetailContext({ view: "task_detail", projectId: "p1" })).toBe(true);
    });

    it("should return false for other views", () => {
      expect(isTaskDetailContext({ view: "kanban", projectId: "p1" })).toBe(false);
    });
  });

  describe("isTicketingContext", () => {
    it("should return true for ticketing view", () => {
      expect(isTicketingContext({ view: "ticketing", projectId: "p1" })).toBe(true);
    });

    it("should return false for non-ticketing views", () => {
      expect(isTicketingContext({ view: "kanban", projectId: "p1" })).toBe(false);
      expect(isTicketingContext({ view: "ideation", projectId: "p1" })).toBe(false);
      expect(isTicketingContext({ view: "activity", projectId: "p1" })).toBe(false);
      expect(isTicketingContext({ view: "agents", projectId: "p1" })).toBe(false);
    });
  });

  describe("isGranolaContext", () => {
    it("should return true for granola view", () => {
      expect(isGranolaContext({ view: "granola", projectId: "p1" })).toBe(true);
    });

    it("should return false for non-granola views", () => {
      expect(isGranolaContext({ view: "kanban", projectId: "p1" })).toBe(false);
      expect(isGranolaContext({ view: "ticketing", projectId: "p1" })).toBe(false);
      expect(isGranolaContext({ view: "github", projectId: "p1" })).toBe(false);
    });
  });
});

describe("Context factory functions", () => {
  describe("createKanbanContext", () => {
    it("should create kanban context without selection", () => {
      const ctx = createKanbanContext("project-123");
      expect(ctx.view).toBe("kanban");
      expect(ctx.projectId).toBe("project-123");
      expect(ctx.selectedTaskId).toBeUndefined();
    });

    it("should create kanban context with selected task", () => {
      const ctx = createKanbanContext("project-123", "task-456");
      expect(ctx.view).toBe("kanban");
      expect(ctx.selectedTaskId).toBe("task-456");
    });
  });

  describe("createIdeationContext", () => {
    it("should create ideation context", () => {
      const ctx = createIdeationContext("project-123", "session-456");
      expect(ctx.view).toBe("ideation");
      expect(ctx.projectId).toBe("project-123");
      expect(ctx.ideationSessionId).toBe("session-456");
    });
  });

  describe("createTaskDetailContext", () => {
    it("should create task detail context", () => {
      const ctx = createTaskDetailContext("project-123", "task-456");
      expect(ctx.view).toBe("task_detail");
      expect(ctx.projectId).toBe("project-123");
      expect(ctx.selectedTaskId).toBe("task-456");
    });
  });

  describe("createProjectContext", () => {
    it("should create project context with specified view", () => {
      const ctx = createProjectContext("project-123", "activity");
      expect(ctx.view).toBe("activity");
      expect(ctx.projectId).toBe("project-123");
    });

    it("should create activity context", () => {
      const ctx = createProjectContext("project-123", "activity");
      expect(ctx.view).toBe("activity");
    });

    it("should accept 'ticketing' as a valid view and create a ticketing context", () => {
      const ctx = createProjectContext("project-123", "ticketing");
      expect(ctx.view).toBe("ticketing");
      expect(ctx.projectId).toBe("project-123");
      expect(isTicketingContext(ctx)).toBe(true);
    });

    it("should accept 'granola' as a valid view and create a Granola context", () => {
      const ctx = createProjectContext("project-123", "granola");
      expect(ctx.view).toBe("granola");
      expect(ctx.projectId).toBe("project-123");
      expect(isGranolaContext(ctx)).toBe(true);
    });
  });
});
