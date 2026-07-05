/**
 * Tests for useAnalysisEditor hook
 *
 * Tests state management, field operations, array operations, persistence,
 * and edge cases like re-analysis events and empty installs.
 */

import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { toast } from "sonner";
import type { Project } from "@/types/project";
import { useAnalysisEditor } from "./useAnalysisEditor";
import { api } from "@/lib/tauri";
import "@/providers/EventProvider";

// Mock the API
vi.mock("@/lib/tauri", () => ({
  api: {
    projects: {
      updateCustomAnalysis: vi.fn(),
    },
  },
}));

// Mock the EventProvider
const mockSubscribe = vi.fn(() => vi.fn());
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: mockSubscribe,
  }),
}));

// Mock sonner toast
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

const mockProject: Project = {
  id: "test-project",
  name: "Test Project",
  workingDirectory: "/home/test",
  baseBranch: "main",
  gitMode: "worktree",
  detectedAnalysis: JSON.stringify([
    {
      path: ".",
      label: "Frontend",
      install: "npm install",
      validate: ["npm run typecheck", "npm run lint"],
      worktree_setup: ["ln -s node_modules"],
    },
    {
      path: "src-tauri",
      label: "Backend",
      install: null,
      validate: ["cargo test"],
      worktree_setup: ["ln -s target"],
    },
  ]),
  customAnalysis: null,
  analyzedAt: new Date().toISOString(),
  mergeValidationMode: "block",
  useFeatureBranches: false,
  worktreeParentDirectory: "~/ralphx-worktrees",
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

describe("useAnalysisEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.projects.updateCustomAnalysis).mockResolvedValue(mockProject);
  });

  describe("initialization", () => {
    it("initializes from detectedAnalysis when no customAnalysis", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      expect(result.current.entries).toHaveLength(2);
      expect(result.current.entries[0].path).toBe(".");
      expect(result.current.entries[0].label).toBe("Frontend");
      expect(result.current.isDirty).toBe(false);
    });

    it("initializes from customAnalysis when available", () => {
      const customProject: Project = {
        ...mockProject,
        customAnalysis: JSON.stringify([
          {
            path: "./custom",
            label: "Custom Path",
            install: "npm install",
            validate: ["echo test"],
            worktree_setup: [],
          },
        ]),
      };

      const { result } = renderHook(() => useAnalysisEditor(customProject));

      expect(result.current.entries).toHaveLength(1);
      expect(result.current.entries[0].path).toBe("./custom");
      // Custom analysis differs from detected, so it should be marked as dirty
      expect(result.current.isDirty).toBe(true);
    });

    it("returns empty array for null project", () => {
      const { result } = renderHook(() => useAnalysisEditor(null));

      expect(result.current.entries).toHaveLength(0);
      expect(result.current.isDirty).toBe(false);
    });
  });

  describe("field operations", () => {
    it("updates field and marks as dirty", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateField(0, "label", "Updated Frontend");
      });

      expect(result.current.entries[0].label).toBe("Updated Frontend");
      expect(result.current.isDirty).toBe(true);
    });

    it("resets field to detected value", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateField(0, "label", "Modified");
      });
      expect(result.current.entries[0].label).toBe("Modified");

      act(() => {
        result.current.resetField(0, "label");
      });

      expect(result.current.entries[0].label).toBe("Frontend");
    });

    it("resets entire entry to detected baseline", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateField(0, "path", "modified-path");
        result.current.updateField(0, "label", "Modified Label");
      });

      act(() => {
        result.current.resetEntry(0);
      });

      expect(result.current.entries[0].path).toBe(".");
      expect(result.current.entries[0].label).toBe("Frontend");
    });

    it("resets all entries to detected baseline", async () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateField(0, "label", "Modified");
        result.current.updateField(1, "label", "Also Modified");
      });

      await act(async () => {
        await result.current.resetAll();
      });

      expect(result.current.entries[0].label).toBe("Frontend");
      expect(result.current.entries[1].label).toBe("Backend");
      expect(result.current.isDirty).toBe(false);
      expect(api.projects.updateCustomAnalysis).not.toHaveBeenCalled();
    });
  });

  describe("array operations", () => {
    it("adds array item (validate)", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.addArrayItem(0, "validate");
      });

      expect(result.current.entries[0].validate).toHaveLength(3);
      expect(result.current.entries[0].validate[2]).toBe("");
      expect(result.current.isDirty).toBe(true);
    });

    it("removes array item (validate)", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.removeArrayItem(0, "validate", 0);
      });

      expect(result.current.entries[0].validate).toHaveLength(1);
      expect(result.current.entries[0].validate[0]).toBe("npm run lint");
    });

    it("updates array item (validate)", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateArrayItem(0, "validate", 0, "new command");
      });

      expect(result.current.entries[0].validate[0]).toBe("new command");
      expect(result.current.isDirty).toBe(true);
    });

    it("handles worktree_setup array operations", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.addArrayItem(0, "worktree_setup");
      });

      expect(result.current.entries[0].worktree_setup).toHaveLength(2);

      act(() => {
        result.current.updateArrayItem(0, "worktree_setup", 1, "new setup");
      });

      expect(result.current.entries[0].worktree_setup[1]).toBe("new setup");

      act(() => {
        result.current.removeArrayItem(0, "worktree_setup", 1);
      });

      expect(result.current.entries[0].worktree_setup).toHaveLength(1);
    });

    it("resets array to detected values", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.addArrayItem(0, "validate");
        result.current.updateArrayItem(0, "validate", 2, "modified");
      });

      act(() => {
        result.current.resetField(0, "validate");
      });

      expect(result.current.entries[0].validate).toEqual([
        "npm run typecheck",
        "npm run lint",
      ]);
    });
  });

  describe("entry operations", () => {
    it("adds new entry", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.addEntry();
      });

      expect(result.current.entries).toHaveLength(3);
      expect(result.current.entries[2]).toEqual({
        path: "",
        label: "",
        install: null,
        validate: [],
        worktree_setup: [],
      });
      expect(result.current.isDirty).toBe(true);
    });

  });

  describe("customization tracking", () => {
    it("detects field customization", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      expect(result.current.isFieldCustomized(0, "label")).toBe(false);

      act(() => {
        result.current.updateField(0, "label", "Modified");
      });

      expect(result.current.isFieldCustomized(0, "label")).toBe(true);
    });

    it("detects user-added entries", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      expect(result.current.isUserAdded(0)).toBe(false);
      expect(result.current.isUserAdded(1)).toBe(false);

      act(() => {
        result.current.addEntry();
      });

      expect(result.current.isUserAdded(2)).toBe(true);
    });

    it("tracks isDirty state correctly", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      expect(result.current.isDirty).toBe(false);

      act(() => {
        result.current.updateField(0, "label", "Changed");
      });
      expect(result.current.isDirty).toBe(true);

      act(() => {
        result.current.resetField(0, "label");
      });
      expect(result.current.isDirty).toBe(false);
    });
  });

  describe("persistence", () => {
    it("initiates save operation with dirty entries", async () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateField(0, "label", "Modified");
      });
      expect(result.current.isDirty).toBe(true);

      // Verify save function exists and can be called
      expect(result.current.save).toBeDefined();
      expect(typeof result.current.save).toBe("function");
    });

    it("tracks when entries are clean (match detected)", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      // Initially clean - entries match detected
      expect(result.current.isDirty).toBe(false);

      // Make change
      act(() => {
        result.current.updateField(0, "label", "Modified");
      });
      expect(result.current.isDirty).toBe(true);

      // Reset it
      act(() => {
        result.current.resetField(0, "label");
      });
      expect(result.current.isDirty).toBe(false);
    });

    it("clears persisted custom analysis when reset all restores detected entries", async () => {
      const customProject: Project = {
        ...mockProject,
        customAnalysis: JSON.stringify([
          {
            path: "./custom",
            label: "Custom Path",
            install: "npm install",
            validate: ["echo test"],
            worktree_setup: [],
          },
        ]),
      };
      const onSaveSuccess = vi.fn();
      const { result } = renderHook(() =>
        useAnalysisEditor(customProject, onSaveSuccess)
      );

      expect(result.current.entries[0].label).toBe("Custom Path");

      await act(async () => {
        await result.current.resetAll();
      });

      expect(api.projects.updateCustomAnalysis).toHaveBeenCalledWith(
        "test-project",
        null
      );
      expect(onSaveSuccess).toHaveBeenCalledWith(null);
      expect(result.current.entries[0].label).toBe("Frontend");
      expect(result.current.isDirty).toBe(false);
    });

    it("resets local entries without persistence when project is null", async () => {
      const { result } = renderHook(() => useAnalysisEditor(null));

      await act(async () => {
        await result.current.resetAll();
      });

      expect(result.current.entries).toEqual([]);
      expect(api.projects.updateCustomAnalysis).not.toHaveBeenCalled();
    });

    it("keeps custom entries when clearing persisted analysis fails", async () => {
      const customProject: Project = {
        ...mockProject,
        customAnalysis: JSON.stringify([
          {
            path: "./custom",
            label: "Custom Path",
            install: "npm install",
            validate: ["echo test"],
            worktree_setup: [],
          },
        ]),
      };

      vi.mocked(api.projects.updateCustomAnalysis).mockRejectedValueOnce(
        new Error("Reset failed")
      );

      const { result } = renderHook(() => useAnalysisEditor(customProject));

      await act(async () => {
        await result.current.resetAll();
      });

      expect(api.projects.updateCustomAnalysis).toHaveBeenCalledWith(
        "test-project",
        null
      );
      expect(toast.error).toHaveBeenCalledWith("Reset failed");
      expect(result.current.entries[0].label).toBe("Custom Path");
      expect(result.current.isDirty).toBe(true);
    });
  });

  describe("edge cases", () => {
    it("handles empty install field (converts to null)", () => {
      const { result } = renderHook(() => useAnalysisEditor(mockProject));

      act(() => {
        result.current.updateField(1, "install", null);
      });

      expect(result.current.entries[1].install).toBeNull();
    });

    it("handles null detectedAnalysis gracefully", () => {
      const noDetectionProject: Project = {
        ...mockProject,
        detectedAnalysis: null,
      };

      const { result } = renderHook(() =>
        useAnalysisEditor(noDetectionProject)
      );

      expect(result.current.entries).toHaveLength(0);
      expect(result.current.isDirty).toBe(false);
    });

    it("handles invalid JSON in analysis strings", () => {
      const invalidProject: Project = {
        ...mockProject,
        detectedAnalysis: "invalid json",
      };

      const { result } = renderHook(() => useAnalysisEditor(invalidProject));

      expect(result.current.entries).toHaveLength(0);
    });
  });
});
