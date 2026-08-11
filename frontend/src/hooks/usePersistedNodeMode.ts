import { useState, useCallback } from "react";
import type { NodeMode } from "../components/TaskGraph/controls/graph-controls";

const STORAGE_KEY = "ralphx:graph:nodeMode";

/**
 * Persists the user's compact/standard node mode preference to localStorage.
 * Returns [value, setter] where value is null for "auto" (no user preference).
 */
export function usePersistedNodeMode(): [NodeMode | null, (mode: NodeMode | null) => void] {
  const [value, setValue] = useState<NodeMode | null>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored === "compact" || stored === "standard") return stored;
    } catch {
      // localStorage unavailable (SSR, private browsing, etc.)
    }
    return null;
  });

  const setPersistedValue = useCallback((mode: NodeMode | null) => {
    try {
      if (mode === null) {
        localStorage.removeItem(STORAGE_KEY);
      } else {
        localStorage.setItem(STORAGE_KEY, mode);
      }
    } catch {
      // localStorage unavailable
    }
    setValue(mode);
  }, []);

  return [value, setPersistedValue];
}
