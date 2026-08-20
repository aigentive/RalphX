/**
 * Mock implementation of @tauri-apps/plugin-dialog for web mode
 *
 * All functions return no-op values since dialogs can't work in browser.
 * This prevents runtime errors when the app runs in web mode.
 */

export interface OpenDialogOptions {
  defaultPath?: string;
  directory?: boolean;
  filters?: { name: string; extensions: string[] }[];
  multiple?: boolean;
  title?: string;
}

export interface SaveDialogOptions {
  defaultPath?: string;
  filters?: { name: string; extensions: string[] }[];
  title?: string;
}

export interface MessageDialogOptions {
  title?: string;
  okLabel?: string;
  cancelLabel?: string;
  kind?: "info" | "warning" | "error";
}

export interface ConfirmDialogOptions {
  title?: string;
  okLabel?: string;
  cancelLabel?: string;
  kind?: "info" | "warning" | "error";
}

const DEFAULT_REPOSITORY_DIRECTORY_PATH = "/Users/test/projects/test-project";
const DEFAULT_PARENT_DIRECTORY_PATH = "/Users/test/projects";

/**
 * Mock open dialog - returns a test path for directory selection.
 *
 * The wizard opens this dialog for two different purposes: picking an
 * existing repository folder (or a new-repository destination folder) vs.
 * picking a parent folder (`NewRepositoryStep`'s "Select Parent Folder",
 * or the worktree-parent browse button). Callers only distinguish these by
 * `title`, so this mock keys off the same signal, following the
 * `window.__mockGhAuthStatus`-style override pattern for Playwright specs
 * that need a specific path: set `window.__mockDialogDirectoryPath`.
 */
export async function open(
  options?: OpenDialogOptions
): Promise<string | string[] | null> {
  console.debug("[mock] dialog.open called");

  // For directory selection (used by ProjectCreationWizard), return a test path
  if (options?.directory) {
    const override = (
      window as Window & { __mockDialogDirectoryPath?: string }
    ).__mockDialogDirectoryPath;
    if (override) {
      console.debug(`[mock] Returning overridden directory: ${override}`);
      return override;
    }

    const isParentPicker = /parent/i.test(options.title ?? "");
    const testPath = isParentPicker
      ? DEFAULT_PARENT_DIRECTORY_PATH
      : DEFAULT_REPOSITORY_DIRECTORY_PATH;
    console.debug(`[mock] Returning test directory: ${testPath}`);
    return testPath;
  }

  // For file selection, return null (user cancelled)
  return null;
}

/**
 * Mock save dialog - returns null (user cancelled)
 */
export async function save(_options?: SaveDialogOptions): Promise<string | null> {
  console.debug("[mock] dialog.save called - returning null");
  return null;
}

/**
 * Mock message dialog - resolves immediately
 */
export async function message(
  _message: string,
  _options?: string | MessageDialogOptions
): Promise<void> {
  console.debug("[mock] dialog.message called");
}

/**
 * Mock ask dialog - returns true (confirmed)
 */
export async function ask(
  _message: string,
  _options?: string | ConfirmDialogOptions
): Promise<boolean> {
  console.debug("[mock] dialog.ask called - returning true");
  return true;
}

/**
 * Mock confirm dialog - returns true (confirmed)
 */
export async function confirm(
  _message: string,
  _options?: string | ConfirmDialogOptions
): Promise<boolean> {
  console.debug("[mock] dialog.confirm called - returning true");
  return true;
}
