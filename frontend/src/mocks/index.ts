/**
 * Tauri plugin mocks for web mode
 *
 * These mocks are used via Vite aliases when running in web mode (--mode web).
 * They provide no-op implementations to prevent runtime errors.
 */

// Re-export all plugin mocks for convenience
export * as dialog from "./tauri-plugin-dialog";
export * as fs from "./tauri-plugin-fs";
export * as process from "./tauri-plugin-process";
export * as updater from "./tauri-plugin-updater";
export * as globalShortcut from "./tauri-plugin-global-shortcut";
export * as notification from "./tauri-plugin-notification";
