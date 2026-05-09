/**
 * TaskPickerDialog - Modal dialog for selecting draft tasks to seed ideation sessions
 *
 * Features:
 * - Displays list of draft (backlog) tasks
 * - Search/filter by task title
 * - On select: returns selected task and closes dialog
 */

import { useState, useMemo } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Search, FileText } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { useTasks } from "@/hooks/useTasks";
import type { Task } from "@/types/task";

// ============================================================================
// Types
// ============================================================================

export interface TaskPickerDialogProps {
  /** Whether the dialog is open */
  isOpen: boolean;
  /** Callback when dialog is closed */
  onClose: () => void;
  /** Callback when a task is selected */
  onSelect: (task: Task) => void;
}

// ============================================================================
// Component
// ============================================================================

export function TaskPickerDialog({
  isOpen,
  onClose,
  onSelect,
}: TaskPickerDialogProps) {
  const [searchQuery, setSearchQuery] = useState("");

  // Get current project
  const activeProjectId = useProjectStore((state) => state.activeProjectId);
  const { data: tasks, isLoading } = useTasks(activeProjectId ?? "");

  // Filter to only draft (backlog) tasks and apply search
  const filteredTasks = useMemo(() => {
    if (!tasks) return [];

    return tasks.filter((task) => {
      // Only show backlog (draft) tasks
      if (task.internalStatus !== "backlog") return false;
      // Only show non-archived tasks
      if (task.archivedAt !== null) return false;
      // Apply search filter
      if (searchQuery) {
        const query = searchQuery.toLowerCase();
        return (
          task.title.toLowerCase().includes(query) ||
          (task.description?.toLowerCase().includes(query) ?? false)
        );
      }
      return true;
    });
  }, [tasks, searchQuery]);

  const handleSelect = (task: Task) => {
    onSelect(task);
    onClose();
    // Reset search on close
    setSearchQuery("");
  };

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      onClose();
      setSearchQuery("");
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent
        className="max-w-md max-h-[70vh] overflow-hidden flex flex-col"
        aria-describedby={undefined}
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <DialogHeader>
          <DialogTitle
            style={{
              color: "var(--text-primary)",
              fontFamily: "SF Pro Display, -apple-system, BlinkMacSystemFont, sans-serif",
            }}
          >
            Select Draft Task
          </DialogTitle>
        </DialogHeader>

        {/* Search input */}
        <div className="relative mt-2">
          <Search
            className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4"
            style={{ color: "var(--text-muted)" }}
          />
          <Input
            placeholder="Search tasks..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="pl-9 h-9 text-sm bg-[var(--bg-surface)] border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            style={{
              outline: "none",
              boxShadow: "none",
            }}
          />
        </div>

        {/* Task list */}
        <div
          className="flex-1 overflow-y-auto mt-3 -mx-6 px-6 min-h-0"
          style={{ maxHeight: "calc(70vh - 140px)" }}
        >
          {isLoading && (
            <div
              className="flex items-center justify-center py-12"
              style={{ color: "var(--text-muted)" }}
            >
              Loading tasks...
            </div>
          )}

          {!isLoading && filteredTasks.length === 0 && (
            <div
              className="flex flex-col items-center justify-center py-12 text-center"
              style={{ color: "var(--text-muted)" }}
            >
              <FileText className="w-8 h-8 mb-3 opacity-50" />
              <p className="text-sm">
                {searchQuery
                  ? "No draft tasks match your search"
                  : "No draft tasks available"}
              </p>
              <p className="text-xs mt-1 opacity-75">
                {searchQuery
                  ? "Try a different search term"
                  : "Create a task in the backlog first"}
              </p>
            </div>
          )}

          {!isLoading && filteredTasks.length > 0 && (
            <div className="space-y-1">
              {filteredTasks.map((task) => (
                <button
                  key={task.id}
                  onClick={() => handleSelect(task)}
                  className="w-full text-left px-3 py-2.5 rounded-lg transition-colors hover:bg-[var(--bg-hover)] group"
                >
                  <div
                    className="text-sm font-medium truncate"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {task.title}
                  </div>
                  {task.description && (
                    <div
                      className="text-xs mt-0.5 truncate"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {task.description}
                    </div>
                  )}
                  <div
                    className="text-[0.625rem] mt-1 uppercase tracking-wide"
                    style={{ color: "var(--text-muted)", opacity: 0.7 }}
                  >
                    {task.category}
                  </div>
                </button>
              ))}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
