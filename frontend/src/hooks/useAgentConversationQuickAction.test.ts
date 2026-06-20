/**
 * Tests for useAgentConversationQuickAction hook
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { useAgentConversationQuickAction } from "./useAgentConversationQuickAction";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import { chatApi } from "@/api/chat";
import { ideationApi } from "@/api/ideation";

// Mock dependencies
vi.mock("@/stores/agentSessionStore");
vi.mock("@/stores/uiStore");
vi.mock("@/api/chat");
vi.mock("@/api/ideation");

describe("useAgentConversationQuickAction", () => {
  const projectId = "test-project-123";

  let mockSetFocusedProject: ReturnType<typeof vi.fn>;
  let mockClearSelection: ReturnType<typeof vi.fn>;
  let mockSetStartConversationDraft: ReturnType<typeof vi.fn>;
  let mockSetCurrentView: ReturnType<typeof vi.fn>;
  let mockOnClose: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();

    // Mock stores
    mockSetFocusedProject = vi.fn();
    mockClearSelection = vi.fn();
    mockSetStartConversationDraft = vi.fn();
    mockSetCurrentView = vi.fn();
    mockOnClose = vi.fn();

    vi.mocked(useAgentSessionStore).mockImplementation(<T,>(selector: (state: {
      setFocusedProject: typeof mockSetFocusedProject;
      clearSelection: typeof mockClearSelection;
      setStartConversationDraft: typeof mockSetStartConversationDraft;
    }) => T): T => {
      const store = {
        setFocusedProject: mockSetFocusedProject,
        clearSelection: mockClearSelection,
        setStartConversationDraft: mockSetStartConversationDraft,
      };
      return selector(store);
    });

    vi.mocked(useUiStore).mockImplementation(<T,>(selector: (state: { setCurrentView: typeof mockSetCurrentView }) => T): T => {
      const store = {
        setCurrentView: mockSetCurrentView,
      };
      return selector(store);
    });

    // Mock APIs
    vi.mocked(chatApi.sendAgentMessage).mockResolvedValue({
      conversationId: "conv-123",
      agentRunId: "run-123",
      isNewConversation: true,
    });

    vi.mocked(ideationApi.sessions.spawnSessionNamer).mockResolvedValue(undefined);
  });

  describe("action properties", () => {
    it("should have correct id", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.id).toBe("agent-conversation");
    });

    it("should have an icon", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.icon).toBeDefined();
      // Lucide icons are components, just verify it's defined and truthy
      expect(result.current.icon).toBeTruthy();
    });

    it("should have correct label", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.label).toBe("Start new agent conversation");
    });

    it("should have correct labels for creating/success/view", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.requiresConfirmation).toBe(false);
      expect(result.current.creatingLabel).toBe("Opening agent composer...");
      expect(result.current.successLabel).toBe("Agent composer ready");
      expect(result.current.viewLabel).toBe("View Composer");
    });
  });

  describe("isVisible", () => {
    it("should return true when query is not empty", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.isVisible("test query")).toBe(true);
      expect(result.current.isVisible("a")).toBe(true);
    });

    it("should return false when query is empty", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.isVisible("")).toBe(false);
      expect(result.current.isVisible("   ")).toBe(false);
    });

    it("should return false when query is only whitespace", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.isVisible("  \t  \n  ")).toBe(false);
    });
  });

  describe("description", () => {
    it("should return query wrapped in quotes", () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));
      expect(result.current.description("Build a user dashboard")).toBe('"Build a user dashboard"');
      expect(result.current.description("test")).toBe('"test"');
    });
  });

  describe("execute", () => {
    it("should prepare an agent composer draft and navigate to Agents", async () => {
      const { result } = renderHook(() =>
        useAgentConversationQuickAction(projectId, { onClose: mockOnClose })
      );

      await result.current.execute("  Build a user dashboard  ");

      expect(mockSetStartConversationDraft).toHaveBeenCalledWith({
        projectId,
        content: "Build a user dashboard",
        mode: "edit",
      });
      expect(mockSetFocusedProject).toHaveBeenCalledWith(projectId);
      expect(mockClearSelection).toHaveBeenCalled();
      expect(mockSetCurrentView).toHaveBeenCalledWith("agents");
      expect(mockOnClose).toHaveBeenCalled();
    });

    it("should not create an ideation session or send the prompt", async () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));

      await result.current.execute("Build a user dashboard");

      expect(chatApi.sendAgentMessage).not.toHaveBeenCalled();
      expect(ideationApi.sessions.spawnSessionNamer).not.toHaveBeenCalled();
    });

    it("should return the project ID for the prepared composer target", async () => {
      const { result } = renderHook(() => useAgentConversationQuickAction(projectId));

      const entityId = await result.current.execute("Build a user dashboard");

      expect(entityId).toBe(projectId);
    });
  });

  describe("navigateTo", () => {
    it("should switch to Agents and clear active conversation selection", () => {
      const { result } = renderHook(() =>
        useAgentConversationQuickAction(projectId, { onClose: mockOnClose })
      );

      result.current.navigateTo(projectId);

      expect(mockSetFocusedProject).toHaveBeenCalledWith(projectId);
      expect(mockClearSelection).toHaveBeenCalled();
      expect(mockSetCurrentView).toHaveBeenCalledWith("agents");
      expect(mockOnClose).toHaveBeenCalled();
    });
  });

  describe("memoization", () => {
    it("should return same action object on re-render when deps don't change", () => {
      const { result, rerender } = renderHook(() => useAgentConversationQuickAction(projectId));

      const firstResult = result.current;
      rerender();
      const secondResult = result.current;

      expect(firstResult).toBe(secondResult);
    });
  });
});
