import { useCallback, useSyncExternalStore } from "react";

const SKILLS_ENABLED_KEY = "ralphx-skills-enabled";

type SkillsSettingsListener = () => void;

const listeners = new Set<SkillsSettingsListener>();

function readPersistedSkillsEnabled(): boolean {
  if (typeof window === "undefined") {
    return true;
  }
  try {
    const storage = window.localStorage;
    if (!storage || typeof storage.getItem !== "function") {
      return true;
    }
    return storage.getItem(SKILLS_ENABLED_KEY) !== "false";
  } catch {
    return true;
  }
}

let skillsEnabled = readPersistedSkillsEnabled();

function subscribe(listener: SkillsSettingsListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): boolean {
  return skillsEnabled;
}

export function setSkillsEnabled(enabled: boolean): void {
  if (skillsEnabled === enabled) {
    return;
  }
  skillsEnabled = enabled;
  try {
    if (typeof window !== "undefined") {
      const storage = window.localStorage;
      if (storage && typeof storage.setItem === "function") {
        storage.setItem(SKILLS_ENABLED_KEY, enabled ? "true" : "false");
      }
    }
  } catch {
    // Keep the in-memory setting active even when persistence is unavailable.
  }
  listeners.forEach((listener) => listener());
}

export function resetSkillsEnabledForTests(enabled = true): void {
  skillsEnabled = enabled;
  try {
    if (typeof window !== "undefined") {
      window.localStorage.removeItem(SKILLS_ENABLED_KEY);
    }
  } catch {
    // Test environments can provide partial localStorage implementations.
  }
  listeners.forEach((listener) => listener());
}

export function useSkillsEnabled(): readonly [boolean, (enabled: boolean) => void] {
  const enabled = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  const setEnabled = useCallback((next: boolean) => {
    setSkillsEnabled(next);
  }, []);
  return [enabled, setEnabled] as const;
}
