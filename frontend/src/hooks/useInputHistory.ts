import { useCallback, useRef } from "react";

const STORAGE_KEY = "ralphx:composer-input-history";
const MAX_ENTRIES = 50;

function loadEntries(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((entry): entry is string => typeof entry === "string");
  } catch {
    return [];
  }
}

function saveEntries(entries: string[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
  } catch {
    // localStorage unavailable
  }
}

function addToEntries(entries: string[], message: string): string[] {
  const trimmed = message.trim();
  if (!trimmed) return entries;
  const filtered = entries.filter((entry) => entry !== trimmed);
  const next = [...filtered, trimmed];
  if (next.length > MAX_ENTRIES) {
    return next.slice(next.length - MAX_ENTRIES);
  }
  return next;
}

function isOnFirstLine(textarea: HTMLTextAreaElement): boolean {
  const pos = textarea.selectionStart;
  const text = textarea.value;
  return !text.slice(0, pos).includes("\n");
}

function isOnLastLine(textarea: HTMLTextAreaElement): boolean {
  const pos = textarea.selectionStart;
  const text = textarea.value;
  return !text.slice(pos).includes("\n");
}

export interface UseInputHistoryOptions {
  setValue: (value: string) => void;
}

export interface UseInputHistoryReturn {
  addEntry: (message: string) => void;
  handleHistoryKeyDown: (
    event: React.KeyboardEvent<HTMLTextAreaElement>,
    currentValue: string
  ) => boolean;
  resetNavigation: () => void;
}

export function useInputHistory({
  setValue,
}: UseInputHistoryOptions): UseInputHistoryReturn {
  const indexRef = useRef(-1);
  const draftRef = useRef<string | null>(null);
  const entriesRef = useRef<string[] | null>(null);

  const getEntries = useCallback((): string[] => {
    if (entriesRef.current === null) {
      entriesRef.current = loadEntries();
    }
    return entriesRef.current;
  }, []);

  const addEntry = useCallback(
    (message: string) => {
      const entries = getEntries();
      const next = addToEntries(entries, message);
      entriesRef.current = next;
      saveEntries(next);
      indexRef.current = -1;
      draftRef.current = null;
    },
    [getEntries]
  );

  const resetNavigation = useCallback(() => {
    indexRef.current = -1;
    draftRef.current = null;
  }, []);

  const handleHistoryKeyDown = useCallback(
    (
      event: React.KeyboardEvent<HTMLTextAreaElement>,
      currentValue: string
    ): boolean => {
      const textarea = event.currentTarget;
      const entries = getEntries();
      if (entries.length === 0) return false;

      if (event.key === "ArrowUp" && isOnFirstLine(textarea)) {
        event.preventDefault();

        if (indexRef.current === -1) {
          draftRef.current = currentValue;
        }

        const nextIndex = indexRef.current + 1;
        if (nextIndex >= entries.length) return true;

        indexRef.current = nextIndex;
        const entryFromEnd = entries[entries.length - 1 - nextIndex]!;
        setValue(entryFromEnd);
        return true;
      }

      if (event.key === "ArrowDown" && isOnLastLine(textarea)) {
        if (indexRef.current <= -1) return false;

        event.preventDefault();
        const nextIndex = indexRef.current - 1;

        if (nextIndex < 0) {
          indexRef.current = -1;
          setValue(draftRef.current ?? "");
          draftRef.current = null;
          return true;
        }

        indexRef.current = nextIndex;
        const entryFromEnd = entries[entries.length - 1 - nextIndex]!;
        setValue(entryFromEnd);
        return true;
      }

      return false;
    },
    [getEntries, setValue]
  );

  return { addEntry, handleHistoryKeyDown, resetNavigation };
}
