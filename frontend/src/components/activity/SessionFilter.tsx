/**
 * SessionFilter - Searchable dropdown for filtering activity by ideation session
 *
 * Uses Popover pattern with search input for selecting sessions.
 * Shows recent sessions (last 15) with search/filter functionality.
 */

import { useState, useMemo, useCallback } from "react";
import { MessageSquare, Search, X, ChevronDown, Check } from "lucide-react";
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
import { useIdeationSessions } from "@/hooks/useIdeation";
import type { IdeationSessionResponse } from "@/api/ideation.types";

// ============================================================================
// Types
// ============================================================================

export interface SessionFilterProps {
  /** Currently selected session ID */
  selectedSessionId: string | null;
  /** Callback when session selection changes */
  onChange: (sessionId: string | null) => void;
}

// ============================================================================
// Component
// ============================================================================

export function SessionFilter({ selectedSessionId, onChange }: SessionFilterProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const activeProjectId = useProjectStore((state) => state.activeProjectId);
  const { data: sessions, isLoading } = useIdeationSessions(activeProjectId ?? "");

  // Get recent sessions (non-archived) sorted by updated_at descending, limited to 15
  const recentSessions = useMemo(() => {
    if (!sessions) return [];
    return sessions
      .filter((session) => session.archivedAt === null)
      .sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime())
      .slice(0, 15);
  }, [sessions]);

  // Filter sessions by search query
  const filteredSessions = useMemo(() => {
    if (!searchQuery.trim()) return recentSessions;
    const query = searchQuery.toLowerCase();
    return recentSessions.filter(
      (session) =>
        (session.title?.toLowerCase().includes(query) ?? false) ||
        session.id.toLowerCase().includes(query)
    );
  }, [recentSessions, searchQuery]);

  // Find selected session for display
  const selectedSession = useMemo(() => {
    if (!selectedSessionId || !sessions) return null;
    return sessions.find((s) => s.id === selectedSessionId) ?? null;
  }, [selectedSessionId, sessions]);

  const handleSelect = useCallback(
    (session: IdeationSessionResponse) => {
      onChange(session.id);
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

  // Format session title for display
  const getSessionDisplayTitle = (session: IdeationSessionResponse): string => {
    if (session.title) return session.title;
    // Fallback to truncated ID if no title
    return `Session ${session.id.slice(0, 8)}...`;
  };

  return (
    <Popover open={isOpen} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className={cn(
            "h-8 text-xs gap-1.5 bg-[var(--bg-elevated)] border-[var(--border-default)] hover:bg-[var(--bg-hover)]",
            selectedSessionId && "border-[var(--accent-primary)]/50"
          )}
        >
          <MessageSquare className="w-3 h-3" />
          {selectedSession ? (
            <span className="max-w-[120px] truncate">{getSessionDisplayTitle(selectedSession)}</span>
          ) : (
            "Session"
          )}
          {selectedSessionId && (
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
              placeholder="Search sessions..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="h-8 pl-8 pr-2 text-xs bg-[var(--bg-surface)] border-[var(--border-subtle)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:ring-1 focus:ring-[var(--accent-primary)]/30"
              style={{ outline: "none", boxShadow: "none" }}
              autoFocus
            />
          </div>
        </div>

        {/* Session list */}
        <ScrollArea className="max-h-64">
          <div className="p-1">
            {isLoading && (
              <div
                className="flex items-center justify-center py-6 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                Loading sessions...
              </div>
            )}

            {!isLoading && filteredSessions.length === 0 && (
              <div
                className="flex flex-col items-center justify-center py-6 text-center"
                style={{ color: "var(--text-muted)" }}
              >
                <MessageSquare className="w-6 h-6 mb-2 opacity-50" />
                <p className="text-xs">
                  {searchQuery ? "No sessions match your search" : "No sessions available"}
                </p>
              </div>
            )}

            {!isLoading && filteredSessions.length > 0 && (
              <div className="space-y-0.5">
                {filteredSessions.map((session) => {
                  const isSelected = session.id === selectedSessionId;
                  return (
                    <button
                      key={session.id}
                      onClick={() => handleSelect(session)}
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
                          <div className="truncate font-medium">{getSessionDisplayTitle(session)}</div>
                          <div
                            className="truncate text-[0.625rem] mt-0.5"
                            style={{ color: "var(--text-muted)" }}
                          >
                            {session.status}
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
