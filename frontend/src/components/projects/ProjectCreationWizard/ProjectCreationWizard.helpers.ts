/**
 * ProjectCreationWizard helpers - validation and utility functions
 */

// ============================================================================
// Form State Interface
// ============================================================================

export interface FormState {
  name: string;
  workingDirectory: string;
  baseBranch: string;
  worktreeParentDirectory: string;
}

export interface FormErrors {
  name?: string;
  workingDirectory?: string;
  baseBranch?: string;
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Generate worktree path from working directory
 * Format: <parentDirectory>/<project-folder-name>
 * Default parent directory: ~/ralphx-worktrees
 */
export function generateWorktreePath(workingDirectory: string, parentDirectory?: string): string {
  const folderName = workingDirectory.split("/").pop() || "project";
  const parent = parentDirectory?.trim() || "~/ralphx-worktrees";
  return `${parent}/${folderName}`;
}

/**
 * Extract folder name from path for auto-inferring project name
 */
export function extractFolderName(path: string): string {
  const folderName = path.split("/").pop() || "";
  return folderName;
}

/**
 * Derive a filesystem-safe folder name from a project name.
 * Used by the Create New Repository step to suggest a destination folder
 * before the user types one explicitly.
 */
export function deriveFolderName(projectName: string): string {
  return projectName
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/**
 * Validate the form and return errors
 * Note: Project name is optional - will be inferred from folder if empty
 */
export function validateForm(form: FormState): FormErrors {
  const errors: FormErrors = {};

  // Project name is now optional - will be inferred from folder
  // Only validate if user explicitly set it to something invalid
  // (not just empty)

  if (!form.workingDirectory.trim()) {
    errors.workingDirectory = "Location is required";
  }

  if (!form.baseBranch.trim()) {
    errors.baseBranch = "Base branch is required";
  }

  return errors;
}
