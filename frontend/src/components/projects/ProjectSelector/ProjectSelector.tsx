/**
 * ProjectSelector - Compact header dropdown for project selection
 *
 * A refined dropdown selector showing current project with git mode indicator.
 * Uses the shared searchable project popover for keyboard-accessible selection.
 *
 * Design: Follows RalphX design system with warm orange accent, SF Pro fonts,
 * 8pt grid, dark theme. Full keyboard accessibility with arrow navigation.
 */

import { useMemo, useCallback } from "react";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";
import { useProjects } from "@/hooks/useProjects";
import { ProjectDropdown } from "./ProjectDropdown";

// ============================================================================
// Props Interface
// ============================================================================

export interface ProjectSelectorProps {
  /** Callback when New Project is selected */
  onNewProject: () => void;
  /** Called immediately before switching to another existing project. */
  onBeforeProjectChange?: ((projectId: string) => void) | undefined;
  /** Optional className for custom styling */
  className?: string;
  /** Dropdown alignment - defaults to center */
  align?: "start" | "center" | "end";
  /** Controlled top-bar menu state. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}

export function ProjectSelector({
  onNewProject,
  onBeforeProjectChange,
  className = "",
  align = "center",
  open,
  onOpenChange,
}: ProjectSelectorProps) {
  // Store state (selection only)
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const selectProject = useProjectStore((s) => s.selectProject);
  const storeActiveProject = useProjectStore(selectActiveProject);

  // Fetch projects directly from backend via TanStack Query.
  // This avoids depending on the Zustand store sync (useEffect in App.tsx)
  // which can lag behind, causing projects to briefly disappear.
  const { data: fetchedProjects } = useProjects();

  // Convert projects to sorted array (most recently updated first)
  const projectList = useMemo(() => {
    if (!fetchedProjects) return [];
    return [...fetchedProjects].sort((a, b) =>
      new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime()
    );
  }, [fetchedProjects]);

  // Resolve active project for trigger label.
  // Prefer fresh query data keyed by activeProjectId; fall back to store snapshot.
  // This avoids stale trigger labels when the selected ID changed but store project
  // records haven't caught up yet.
  const activeProject = useMemo(() => {
    if (!activeProjectId) return null;
    const fromQuery = fetchedProjects?.find((p) => p.id === activeProjectId);
    if (fromQuery) return fromQuery;
    if (storeActiveProject?.id === activeProjectId) return storeActiveProject;
    return null;
  }, [activeProjectId, fetchedProjects, storeActiveProject]);

  const handleSelectProject = useCallback(
    (projectId: string) => {
      if (projectId !== activeProjectId) {
        onBeforeProjectChange?.(projectId);
      }
      selectProject(projectId);
    },
    [activeProjectId, onBeforeProjectChange, selectProject]
  );

  return (
    <ProjectDropdown
      projects={projectList}
      value={activeProjectId}
      selectedProject={activeProject}
      onValueChange={(projectId) => {
        if (projectId) {
          handleSelectProject(projectId);
        }
      }}
      onNewProject={onNewProject}
      className={className}
      align={align}
      {...(open !== undefined ? { open } : {})}
      {...(onOpenChange ? { onOpenChange } : {})}
      variant="navbar"
      placeholder="Select Project"
      testId="project-selector-trigger"
      dropdownTestId="project-selector-dropdown"
      listTestId="project-selector-list"
      searchTestId="project-selector-search"
      newProjectTestId="new-project-option"
      showMoreTestId="project-selector-show-more"
      projectOptionTestId={(project) => `project-option-${project.id}`}
    />
  );
}
