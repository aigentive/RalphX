/**
 * TaskFilter - Searchable dropdown for filtering activity by task
 *
 * Uses Popover pattern with search input for selecting tasks.
 * Shows recent tasks (last 15) with search/filter functionality.
 */

import { useState, useMemo, useCallback } from "react";
import { ListTodo, Search, X, ChevronDown, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import { useProjectStore } from "@/stores/projectStore";
import { useTasks } from "@/hooks/useTasks";
import type { Task } from "@/types/task";

// ============================================================================
// Types
// ============================================================================

export interface TaskFilterProps {
  /** Currently selected task ID */
  selectedTaskId: string | null;
  /** Callback when task selection changes */
  onChange: (taskId: string | null) => void;
}

// ============================================================================
// Component
// ============================================================================

export function TaskFilter({ selectedTaskId, onChange }: TaskFilterProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const activeProjectId = useProjectStore((state) => state.activeProjectId);
  const { data: tasks, isLoading } = useTasks(activeProjectId ?? "");

  // Get recent tasks (non-archived) sorted by updated_at descending, limited to 15
  const recentTasks = useMemo(() => {
    if (!tasks) return [];
    return tasks
      .filter((task) => task.archivedAt === null)
      .sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime())
      .slice(0, 15);
  }, [tasks]);

  // Filter tasks by search query
  const filteredTasks = useMemo(() => {
    if (!searchQuery.trim()) return recentTasks;
    const query = searchQuery.toLowerCase();
    return recentTasks.filter(
      (task) =>
        task.title.toLowerCase().includes(query) ||
        (task.description?.toLowerCase().includes(query) ?? false)
    );
  }, [recentTasks, searchQuery]);

  // Find selected task for display
  const selectedTask = useMemo(() => {
    if (!selectedTaskId || !tasks) return null;
    return tasks.find((t) => t.id === selectedTaskId) ?? null;
  }, [selectedTaskId, tasks]);

  const handleSelect = useCallback(
    (task: Task) => {
      onChange(task.id);
      setIsOpen(false);
      setSearchQuery("");
    },
    [onChange]
  );

  const handleClear = useCallback(() => {
    onChange(null);
    setSearchQuery("");
  }, [onChange]);

  const handleOpenChange = useCallback((open: boolean) => {
    setIsOpen(open);
    if (!open) {
      setSearchQuery("");
    }
  }, []);

  return (
    <Popover open={isOpen} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className={cn(
            "h-8 text-xs gap-1.5 bg-[var(--bg-elevated)] border-[var(--border-default)] hover:bg-[var(--bg-hover)]",
            selectedTaskId && "border-[var(--accent-primary)]/50"
          )}
        >
          <ListTodo className="w-3 h-3" />
          {selectedTask ? (
            <span className="max-w-[120px] truncate">{selectedTask.title}</span>
          ) : (
            "Task"
          )}
          {selectedTaskId && (
            <span
              className="ml-0.5 px-1 py-0.5 rounded-full bg-[var(--accent-primary)] text-white text-[0.625rem] cursor-pointer hover:bg-[var(--accent-primary)]/80"
              onClick={(e) => {
                e.stopPropagation();
                handleClear();
              }}
            >
              <X className="w-2.5 h-2.5" />
            </span>
          )}
          <ChevronDown className="w-3 h-3 ml-1" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-72 p-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
        }}
      >
        {/* Search input */}
        <div className="p-2 border-b border-[var(--border-subtle)]">
          <div className="relative">
            <Search
              className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              placeholder="Search tasks..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-8 pl-8 pr-2 text-xs bg-[var(--bg-surface)] border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
              style={{ outline: "none", boxShadow: "none" }}
              autoFocus
            />
          </div>
        </div>

        {/* Task list */}
        <ScrollArea className="max-h-64">
          <div className="p-1">
            {isLoading && (
              <div
                className="flex items-center justify-center py-6 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                Loading tasks...
              </div>
            )}

            {!isLoading && filteredTasks.length === 0 && (
              <div
                className="flex flex-col items-center justify-center py-6 text-center"
                style={{ color: "var(--text-muted)" }}
              >
                <ListTodo className="w-6 h-6 mb-2 opacity-50" />
                <p className="text-xs">
                  {searchQuery ? "No tasks match your search" : "No tasks available"}
                </p>
              </div>
            )}

            {!isLoading && filteredTasks.length > 0 && (
              <div className="space-y-0.5">
                {filteredTasks.map((task) => {
                  const isSelected = task.id === selectedTaskId;
                  return (
                    <button
                      key={task.id}
                      onClick={() => handleSelect(task)}
                      className={cn(
                        "w-full text-left px-2 py-1.5 rounded-md transition-colors text-xs",
                        isSelected
                          ? "bg-[var(--accent-primary)]/10 text-[var(--accent-primary)]"
                          : "hover:bg-[var(--bg-hover)] text-[var(--text-primary)]"
                      )}
                    >
                      <div className="flex items-center gap-2">
                        {isSelected && <Check className="w-3 h-3 flex-shrink-0" />}
                        <div className="flex-1 min-w-0">
                          <div className="truncate font-medium">{task.title}</div>
                          <div
                            className="truncate text-[0.625rem] mt-0.5"
                            style={{ color: "var(--text-muted)" }}
                          >
                            {task.internalStatus}
                          </div>
                        </div>
                      </div>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        </ScrollArea>
      </PopoverContent>
    </Popover>
  );
}
