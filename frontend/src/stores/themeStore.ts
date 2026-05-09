/**
 * Theme store — controls active theme + motion + font scale.
 *
 * Persists to localStorage and mirrors state to `<html>` data attributes so CSS
 * selectors in globals.css pick up the change:
 *   - data-theme="dark" | "light" | "high-contrast"
 *   - data-motion="reduce" (optional override)
 *   - data-font-scale="lg" | "xl" (optional bump)
 *
 * A tiny inline script in index.html applies these attributes BEFORE React
 * hydrates to avoid flash-of-wrong-theme (FOWT).
 *
 * Spec: specs/design/theme-architecture.md
 */

import { create } from "zustand";

export type ThemeName = "dark" | "light" | "high-contrast";
export type MotionPreference = "system" | "reduce";
export type FontScale = "default" | "lg" | "xl";

const THEME_KEY = "ralphx-theme";
const MOTION_KEY = "ralphx-motion";
const FONT_SCALE_KEY = "ralphx-font-scale";

function safeGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeSet(key: string, value: string | null): void {
  try {
    if (value === null) {
      localStorage.removeItem(key);
    } else {
      localStorage.setItem(key, value);
    }
  } catch {
    /* no-op */
  }
}

export function migrateThemePreference(raw: string | null): ThemeName | null {
  if (raw === "high-contrast") return "high-contrast";
  if (raw === "light") return "light";
  if (raw === "dark") return "dark";
  return null;
}

function loadTheme(): ThemeName {
  const raw = safeGet(THEME_KEY);
  const migrated = migrateThemePreference(raw);
  if (migrated) return migrated;
  if (raw !== null) safeSet(THEME_KEY, null);
  return "dark";
}

function loadMotion(): MotionPreference {
  const v = safeGet(MOTION_KEY);
  return v === "reduce" ? "reduce" : "system";
}

function loadFontScale(): FontScale {
  const v = safeGet(FONT_SCALE_KEY);
  if (v === "lg" || v === "xl") return v;
  return "default";
}

function applyThemeAttr(theme: ThemeName): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  // Always set the attribute explicitly — including "dark" — so the visual
  // state never drifts from React state due to partial removeAttribute calls
  // or bootstrap re-infering from OS preference. CSS aliases :root and
  // [data-theme="dark"] to the same token definitions.
  root.setAttribute("data-theme", theme);
  // Defensive cleanup — shadcn's compatibility block keys off a `.dark`
  // class. If another library/extension set it we ensure it only sticks
  // when the active theme is "dark" so CSS cascade stays deterministic.
  if (theme === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
}

function applyMotionAttr(motion: MotionPreference): void {
  if (typeof document === "undefined") return;
  if (motion === "reduce") {
    document.documentElement.setAttribute("data-motion", "reduce");
  } else {
    document.documentElement.removeAttribute("data-motion");
  }
}

function applyFontScaleAttr(scale: FontScale): void {
  if (typeof document === "undefined") return;
  if (scale === "default") {
    document.documentElement.removeAttribute("data-font-scale");
  } else {
    document.documentElement.setAttribute("data-font-scale", scale);
  }
}

interface ThemeState {
  theme: ThemeName;
  motion: MotionPreference;
  fontScale: FontScale;
  setTheme: (theme: ThemeName) => void;
  setMotion: (motion: MotionPreference) => void;
  setFontScale: (scale: FontScale) => void;
}

declare global {
  interface Window {
    __themeStore?: unknown;
  }
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: loadTheme(),
  motion: loadMotion(),
  fontScale: loadFontScale(),
  setTheme: (theme) => {
    // Always persist the explicit choice — including "dark" — so page reload
    // doesn't re-infer from OS preference and override the user's pick.
    safeSet(THEME_KEY, theme);
    applyThemeAttr(theme);
    set({ theme });
  },
  setMotion: (motion) => {
    safeSet(MOTION_KEY, motion === "system" ? null : motion);
    applyMotionAttr(motion);
    set({ motion });
  },
  setFontScale: (scale) => {
    safeSet(FONT_SCALE_KEY, scale === "default" ? null : scale);
    applyFontScaleAttr(scale);
    set({ fontScale: scale });
  },
}));

/**
 * Initialise DOM attributes from the persisted store state on app mount. Call
 * once from the app root so the React state and the DOM attributes stay in
 * sync even if the inline bootstrap script in index.html was missed.
 */
export function syncThemeAttributesFromStore(): void {
  const { theme, motion, fontScale } = useThemeStore.getState();
  applyThemeAttr(theme);
  applyMotionAttr(motion);
  applyFontScaleAttr(fontScale);
  // Expose for devtools debugging: `window.__themeStore.getState()` returns
  // the current theme/motion/fontScale. Dev only — helps when
  // a user reports theme desync and we need to confirm the store vs DOM.
  if (typeof window !== "undefined") {
    window.__themeStore = useThemeStore;
  }
}
