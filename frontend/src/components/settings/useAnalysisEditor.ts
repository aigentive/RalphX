/**
 * useAnalysisEditor - State management hook for inline-editable analysis entries
 *
 * Features:
 * - Initializes from customAnalysis (if set) or detectedAnalysis
 * - Tracks detected entries as baseline for reset/diff operations
 * - Field/array/entry CRUD operations
 * - Per-field reset to detected baseline
 * - Reset All clears the persisted custom override
 * - Dirty tracking and persistence via api.projects.updateCustomAnalysis
 * - Handles project:analysis_complete event to refresh baseline
 */

/* eslint-disable react-hooks/preserve-manual-memoization -- this hook intentionally exposes stable command callbacks to consumers; the upgraded React Compiler lint now flags these setter wrappers even though their runtime contract is deliberate */

import { useState, useCallback, useEffect } from "react";
import { api } from "@/lib/tauri";
import { useEventBus } from "@/providers/EventProvider";
import type { Project } from "@/types/project";
import { toast } from "sonner";

/**
 * Shape of a single analysis entry
 */
export interface AnalysisEntry {
  path: string;
  label: string;
  install: string | null;
  validate: string[];
  worktree_setup: string[];
}

/**
 * Return type for the hook
 */
export interface UseAnalysisEditorReturn {
  // State
  entries: AnalysisEntry[];
  isDirty: boolean;
  isSaving: boolean;

  // Field operations
  updateField<K extends keyof AnalysisEntry>(
    entryIdx: number,
    field: K,
    value: AnalysisEntry[K]
  ): void;
  resetField(entryIdx: number, field: keyof AnalysisEntry): void;
  resetEntry(entryIdx: number): void;
  resetAll(): Promise<void>;

  // Array operations (validate[], worktree_setup[])
  addArrayItem(entryIdx: number, field: "validate" | "worktree_setup"): void;
  removeArrayItem(
    entryIdx: number,
    field: "validate" | "worktree_setup",
    itemIdx: number
  ): void;
  updateArrayItem(
    entryIdx: number,
    field: "validate" | "worktree_setup",
    itemIdx: number,
    value: string
  ): void;

  // Entry operations
  addEntry(): void;

  // Persistence
  save(): Promise<void>;

  // Queries
  isFieldCustomized(entryIdx: number, field: keyof AnalysisEntry): boolean;
  isUserAdded(entryIdx: number): boolean;
}

function parseAnalysisEntries(json: string | null): AnalysisEntry[] {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function deepClone<T>(obj: T): T {
  return JSON.parse(JSON.stringify(obj));
}

function entriesEqual(a: AnalysisEntry[], b: AnalysisEntry[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function useAnalysisEditor(
  project: Project | null,
  onSaveSuccess?: (customAnalysis: string | null) => void
): UseAnalysisEditorReturn {
  const bus = useEventBus();

  // Initialize from customAnalysis or detectedAnalysis
  const detectedEntries = parseAnalysisEntries(project?.detectedAnalysis ?? null);
  const customEntries = parseAnalysisEntries(project?.customAnalysis ?? null);
  const initialEntries = customEntries.length > 0 ? customEntries : detectedEntries;

  const [entries, setEntries] = useState<AnalysisEntry[]>(deepClone(initialEntries));
  const [detectedBaseline, setDetectedBaseline] =
    useState<AnalysisEntry[]>(deepClone(detectedEntries));
  const [isSaving, setIsSaving] = useState(false);

  // Re-initialize when project changes
  useEffect(() => {
    const detected = parseAnalysisEntries(project?.detectedAnalysis ?? null);
    const custom = parseAnalysisEntries(project?.customAnalysis ?? null);
    const initial = custom.length > 0 ? custom : detected;

    setEntries(deepClone(initial));
    setDetectedBaseline(deepClone(detected));
  }, [project?.id, project?.customAnalysis, project?.detectedAnalysis]);

  // Listen for analysis_complete event to update detected baseline
  useEffect(() => {
    const unsub = bus.subscribe<{
      project_id: string;
      detected_analysis: string | null;
    }>("project:analysis_complete", (payload) => {
      if (project && payload.project_id === project.id) {
        const newDetected = parseAnalysisEntries(payload.detected_analysis);
        setDetectedBaseline(deepClone(newDetected));
        // Don't modify entries — preserve unsaved edits
        toast.info("Analysis baseline updated");
      }
    });

    return () => unsub();
  }, [bus, project]);

  // Compute isDirty: true if entries differ from detected baseline
  const isDirty = !entriesEqual(entries, detectedBaseline);

  // Field operations
  const updateField = useCallback(
    <K extends keyof AnalysisEntry>(entryIdx: number, field: K, value: AnalysisEntry[K]) => {
      setEntries((prev) => {
        const next = deepClone(prev);
        if (next[entryIdx]) {
          next[entryIdx][field] = value;
        }
        return next;
      });
    },
    []
  );

  const resetField = useCallback((entryIdx: number, field: keyof AnalysisEntry) => {
    setEntries((prev) => {
      const next = deepClone(prev);
      const detected = detectedBaseline[entryIdx];
      if (next[entryIdx] && detected) {
        (next[entryIdx] as unknown as Record<string, unknown>)[field] = deepClone((detected as unknown as Record<string, unknown>)[field]);
      }
      return next;
    });
  }, [detectedBaseline]);

  const resetEntry = useCallback(
    (entryIdx: number) => {
      setEntries((prev) => {
        const next = deepClone(prev);
        if (next[entryIdx] && detectedBaseline[entryIdx]) {
          next[entryIdx] = deepClone(detectedBaseline[entryIdx]!);
        }
        return next;
      });
    },
    [detectedBaseline]
  );

  const resetAll = useCallback(async () => {
    const resetEntries = deepClone(detectedBaseline);

    if (!project) {
      setEntries(resetEntries);
      return;
    }

    if (project.customAnalysis === null) {
      setEntries(resetEntries);
      return;
    }

    setIsSaving(true);
    try {
      await api.projects.updateCustomAnalysis(project.id, null);
      setEntries(resetEntries);
      toast.success("Custom override cleared");

      if (onSaveSuccess) {
        onSaveSuccess(null);
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to reset analysis settings");
    } finally {
      setIsSaving(false);
    }
  }, [project, detectedBaseline, onSaveSuccess]);

  // Array operations
  const addArrayItem = useCallback(
    (entryIdx: number, field: "validate" | "worktree_setup") => {
      setEntries((prev) => {
        const next = deepClone(prev);
        if (next[entryIdx]) {
          next[entryIdx][field].push("");
        }
        return next;
      });
    },
    []
  );

  const removeArrayItem = useCallback(
    (entryIdx: number, field: "validate" | "worktree_setup", itemIdx: number) => {
      setEntries((prev) => {
        const next = deepClone(prev);
        if (next[entryIdx]) {
          next[entryIdx][field].splice(itemIdx, 1);
        }
        return next;
      });
    },
    []
  );

  const updateArrayItem = useCallback(
    (entryIdx: number, field: "validate" | "worktree_setup", itemIdx: number, value: string) => {
      setEntries((prev) => {
        const next = deepClone(prev);
        if (next[entryIdx]) {
          next[entryIdx][field][itemIdx] = value;
        }
        return next;
      });
    },
    []
  );

  // Entry operations
  const addEntry = useCallback(() => {
    setEntries((prev) => [
      ...prev,
      {
        path: "",
        label: "",
        install: null,
        validate: [],
        worktree_setup: [],
      },
    ]);
  }, []);

  // Query helpers
  const isFieldCustomized = useCallback(
    (entryIdx: number, field: keyof AnalysisEntry): boolean => {
      const entry = entries[entryIdx];
      const detected = detectedBaseline[entryIdx];

      if (!detected || !entry) return true; // User-added entries are always customized
      return JSON.stringify(entry[field]) !== JSON.stringify(detected[field]);
    },
    [entries, detectedBaseline]
  );

  const isUserAdded = useCallback(
    (entryIdx: number): boolean => {
      return !detectedBaseline[entryIdx];
    },
    [detectedBaseline]
  );

  // Persistence
  const save = useCallback(async () => {
    if (!project) return;

    setIsSaving(true);
    try {
      // If entries match detected, send null to clear override
      const payload = entriesEqual(entries, detectedBaseline) ? null : JSON.stringify(entries);

      await api.projects.updateCustomAnalysis(project.id, payload);
      toast.success(payload ? "Analysis settings saved" : "Custom override cleared");

      // Update local state to reflect saved state
      setDetectedBaseline(deepClone(entries));

      // Notify parent component of successful save
      if (onSaveSuccess) {
        onSaveSuccess(payload);
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to save analysis settings");
    } finally {
      setIsSaving(false);
    }
  }, [project, entries, detectedBaseline, onSaveSuccess]);

  return {
    entries,
    isDirty,
    isSaving,
    updateField,
    resetField,
    resetEntry,
    resetAll,
    addArrayItem,
    removeArrayItem,
    updateArrayItem,
    addEntry,
    save,
    isFieldCustomized,
    isUserAdded,
  };
}
