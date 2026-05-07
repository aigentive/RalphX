import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { Check, ChevronDown, GitPullRequest, Search } from "lucide-react";
import { useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi, type ChatMessageResponse, type ConversationMessagesPageResponse } from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { agentConversationKeys } from "@/components/agents/useProjectAgentConversations";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { chatKeys, useConversationHistoryWindow } from "@/hooks/useChat";
import { cn } from "@/lib/utils";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { useThemeStore, type FontScale } from "@/stores/themeStore";
import type { ViewType } from "@/types/chat";
import type { ChatConversation } from "@/types/chat-conversation";

import { ThemeSelector } from "./ThemeSelector";

interface AppTopBarProps {
  currentView: ViewType;
  pendingReviewCount: number;
  reviewsPanelOpen: boolean;
  onToggleReviewsPanel: () => void;
}

const VIEW_LABELS: Partial<Record<ViewType, string>> = {
  agents: "Agents",
  ideation: "Ideation",
  graph: "Graph",
  kanban: "Kanban",
  insights: "Insights",
  extensibility: "Extensibility",
  activity: "Activity",
  task_detail: "Task",
};

const FONT_SCALE_OPTIONS: Array<{ value: FontScale; label: string }> = [
  { value: "default", label: "100%" },
  { value: "lg", label: "110%" },
  { value: "xl", label: "125%" },
];

function viewLabel(view: ViewType): string {
  return VIEW_LABELS[view] ?? "Workspace";
}

function breadcrumbItems(
  currentView: ViewType,
  projectName: string | null,
  agentConversationTitle: string | null,
): string[] {
  if (currentView === "agents") {
    return ["Workspace", "Agents", agentConversationTitle ?? "New run"];
  }

  if (currentView === "kanban") {
    return ["Workspace", projectName ?? "Project", "Tasks"];
  }

  return ["Workspace", viewLabel(currentView)];
}

interface FontScaleSelectorProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

type ConversationQueryData = {
  conversation: ChatConversation;
  messages: ChatMessageResponse[];
};

function updateConversationTitleInCache(
  queryClient: ReturnType<typeof useQueryClient>,
  conversation: ChatConversation,
  title: string,
) {
  const updatedAt = new Date().toISOString();
  const updateConversation = (item: ChatConversation): ChatConversation =>
    item.id === conversation.id
      ? {
          ...item,
          title,
          updatedAt,
        }
      : item;

  queryClient.setQueryData<ConversationQueryData>(
    chatKeys.conversation(conversation.id),
    (oldData) =>
      oldData
        ? {
            ...oldData,
            conversation: updateConversation(oldData.conversation),
          }
        : oldData,
  );

  queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
    chatKeys.conversationHistory(conversation.id),
    (oldData) =>
      oldData
        ? {
            ...oldData,
            pages: oldData.pages.map((page) => ({
              ...page,
              conversation: updateConversation(page.conversation),
            })),
          }
        : oldData,
  );

  queryClient.setQueryData<ChatConversation[]>(
    chatKeys.conversationList(conversation.contextType, conversation.contextId),
    (oldData) => oldData?.map(updateConversation),
  );
}

interface EditableAgentBreadcrumbTitleProps {
  conversation: ChatConversation;
  title: string;
}

function EditableAgentBreadcrumbTitle({
  conversation,
  title,
}: EditableAgentBreadcrumbTitleProps) {
  const queryClient = useQueryClient();
  const [isEditing, setIsEditing] = useState(false);
  const [draftTitle, setDraftTitle] = useState(title);
  const commitInFlightRef = useRef(false);

  useEffect(() => {
    if (!isEditing) {
      setDraftTitle(title);
    }
  }, [isEditing, title]);

  const commitTitle = useCallback(async () => {
    if (commitInFlightRef.current) {
      return;
    }

    const trimmed = draftTitle.trim();
    if (!trimmed || trimmed === title) {
      setDraftTitle(title);
      setIsEditing(false);
      return;
    }

    commitInFlightRef.current = true;
    updateConversationTitleInCache(queryClient, conversation, trimmed);
    setIsEditing(false);

    try {
      if (conversation.contextType === "ideation") {
        await Promise.all([
          chatApi.updateConversationTitle(conversation.id, trimmed),
          ideationApi.sessions.updateTitle(conversation.contextId, trimmed),
        ]);
      } else {
        await chatApi.updateConversationTitle(conversation.id, trimmed);
      }
      void queryClient.invalidateQueries({ queryKey: chatKeys.conversations() });
      void queryClient.invalidateQueries({ queryKey: agentConversationKeys.all });
    } catch (error) {
      updateConversationTitleInCache(queryClient, conversation, title);
      toast.error(error instanceof Error ? error.message : "Failed to rename session");
    } finally {
      commitInFlightRef.current = false;
    }
  }, [conversation, draftTitle, queryClient, title]);

  if (isEditing) {
    return (
      <input
        value={draftTitle}
        onChange={(event) => setDraftTitle(event.target.value)}
        onBlur={() => void commitTitle()}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void commitTitle();
          }
          if (event.key === "Escape") {
            event.preventDefault();
            setDraftTitle(title);
            setIsEditing(false);
          }
        }}
        className="h-6 max-w-[38ch] rounded-[4px] border border-transparent bg-transparent px-0.5 text-[0.7812rem] font-medium outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
        style={{
          width: `${Math.max(12, Math.min(38, draftTitle.length + 1))}ch`,
          color: "var(--text-primary)",
          WebkitAppRegion: "no-drag",
        } as CSSProperties}
        aria-label="Rename agent conversation"
        autoFocus
      />
    );
  }

  return (
    <button
      type="button"
      className="max-w-[42ch] truncate rounded-[4px] text-left font-medium outline-none hover:text-[var(--text-primary)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
      style={{
        color: "var(--text-primary)",
        WebkitAppRegion: "no-drag",
      } as CSSProperties}
      aria-label="Rename agent conversation"
      data-testid="agent-breadcrumb-title"
      data-theme-button-skip="true"
      onClick={() => setIsEditing(true)}
    >
      {title}
    </button>
  );
}

function FontScaleSelector({ open, onOpenChange }: FontScaleSelectorProps) {
  const fontScale = useThemeStore((s) => s.fontScale);
  const setFontScale = useThemeStore((s) => s.setFontScale);
  const current =
    FONT_SCALE_OPTIONS.find((option) => option.value === fontScale) ?? FONT_SCALE_OPTIONS[0]!;

  return (
    <div className="relative inline-flex shrink-0" data-testid="font-scale-selector">
      <button
        type="button"
        className="inline-flex h-8 items-center gap-1.5 rounded-[6px] border px-2.5 text-[0.7812rem] font-medium transition-colors duration-150 outline-none hover:border-[var(--border-strong)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
        style={{
          backgroundColor: open ? "var(--bg-hover)" : "var(--bg-elevated)",
          borderColor: open ? "var(--border-strong)" : "var(--border-default)",
          color: open ? "var(--text-primary)" : "var(--text-secondary)",
        }}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Font size · ${current.label}`}
        data-testid="font-scale-selector-trigger"
        onClick={() => onOpenChange(!open)}
      >
        <span className="inline-flex items-baseline gap-px text-[0.8125rem] font-semibold leading-none tracking-normal">
          A<b className="text-[0.9375rem]">a</b>
        </span>
        <span>{current.label}</span>
        <ChevronDown
          className={cn("h-3 w-3 shrink-0 transition-transform duration-150", open && "rotate-180")}
          style={{ color: open ? "var(--text-secondary)" : "var(--text-muted)" }}
        />
      </button>

      {open && (
        <div
          role="menu"
          aria-label="Font size"
          data-testid="font-scale-selector-menu"
          className="absolute right-0 top-[calc(100%+6px)] z-[60] flex min-w-[112px] flex-col gap-px rounded-[8px] border p-1 shadow-lg"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-default)",
            boxShadow: "var(--shadow-lg)",
          }}
        >
          {FONT_SCALE_OPTIONS.map((option) => {
            const isActive = option.value === fontScale;

            return (
              <button
                key={option.value}
                type="button"
                role="menuitemradio"
                aria-checked={isActive}
                data-testid={`font-scale-option-${option.value}`}
                className="inline-flex items-center gap-2 rounded-[6px] px-2 py-1.5 text-left text-[0.7812rem] font-medium transition-colors duration-150 outline-none hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
                style={{ color: isActive ? "var(--text-primary)" : "var(--text-secondary)" }}
                onClick={() => {
                  setFontScale(option.value);
                  onOpenChange(false);
                }}
              >
                <span className="flex-1">{option.label}</span>
                <Check
                  className="h-3 w-3 shrink-0"
                  style={{ color: "var(--accent-primary)", opacity: isActive ? 1 : 0 }}
                />
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

export function AppTopBar({
  currentView,
  pendingReviewCount,
  reviewsPanelOpen,
  onToggleReviewsPanel,
}: AppTopBarProps) {
  const activeProject = useProjectStore(selectActiveProject);
  const selectedAgentConversationId = useAgentSessionStore((s) => s.selectedConversationId);
  const selectedAgentConversation = useConversationHistoryWindow(selectedAgentConversationId, {
    enabled: currentView === "agents" && Boolean(selectedAgentConversationId),
    pageSize: 1,
  });
  const [activeMenu, setActiveMenu] = useState<"theme" | "font" | null>(null);
  const agentConversation =
    currentView === "agents" && selectedAgentConversationId
      ? selectedAgentConversation.data?.conversation ?? null
      : null;
  const agentConversationTitle =
    agentConversation
      ? agentConversation.title?.trim() || "Untitled agent"
      : null;
  const crumbs = breadcrumbItems(
    currentView,
    activeProject?.name ?? null,
    agentConversationTitle,
  );
  const reviewsLabel =
    pendingReviewCount > 0
      ? `Reviews · ${pendingReviewCount} pending`
      : "Reviews";

  return (
    <header
      className="fixed left-0 right-0 top-0 z-50 flex h-12 select-none items-center justify-between gap-2 border-b pr-4 pl-[88px]"
      style={{
        backgroundColor: "var(--app-navbar-bg)",
        borderBottomColor: "var(--app-navbar-border)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
      }}
      data-tauri-drag-region
      data-testid="app-header"
    >
      <nav
        className="inline-flex min-w-0 items-center gap-2 text-[0.7812rem]"
        aria-label="Breadcrumb"
        style={{ color: "var(--text-muted)" }}
      >
        {crumbs.map((item, index) => {
          const isLast = index === crumbs.length - 1;

          return (
            <span key={`${item}-${index}`} className="inline-flex items-center gap-2">
              <span
                className={cn(isLast && "font-medium")}
                style={{ color: isLast ? "var(--text-primary)" : "var(--text-muted)" }}
              >
                {isLast && agentConversation ? (
                  <EditableAgentBreadcrumbTitle
                    conversation={agentConversation}
                    title={agentConversationTitle ?? "Untitled agent"}
                  />
                ) : (
                  item
                )}
              </span>
              {!isLast && (
                <span aria-hidden="true" style={{ color: "var(--text-subtle, var(--text-muted))" }}>
                  /
                </span>
              )}
            </span>
          );
        })}
      </nav>

      <div
        className="ml-auto inline-flex items-center gap-2"
        style={{ WebkitAppRegion: "no-drag" } as CSSProperties}
      >
        <button
          type="button"
          disabled
          aria-disabled="true"
          aria-hidden="true"
          tabIndex={-1}
          className="inline-flex h-8 w-[320px] items-center gap-2.5 rounded-[6px] border px-3 text-[0.8125rem] outline-none"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-default)",
            color: "var(--text-secondary)",
            display: "none",
          }}
          aria-label="Search runs, projects, agents (coming soon)"
          data-testid="topbar-command-search"
          title="Coming soon"
        >
          <Search className="h-3.5 w-3.5 shrink-0" />
          <span className="flex-1 truncate text-left" style={{ color: "var(--text-muted)" }}>
            Search runs, projects, agents...
          </span>
          <kbd
            className="rounded-[4px] border px-1.5 py-px text-[0.6562rem] font-medium leading-none"
            style={{
              backgroundColor: "var(--bg-hover)",
              borderColor: "var(--border-default)",
              color: "var(--text-secondary)",
              fontFamily:
                "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
            }}
          >
            ⌘K
          </kbd>
        </button>

        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="relative grid h-8 w-8 place-items-center rounded-[6px] border transition-colors duration-150 outline-none hover:border-[var(--border-default)] hover:bg-[var(--bg-elevated)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
              style={{
                borderColor: "transparent",
                color: reviewsPanelOpen ? "var(--text-primary)" : "var(--text-muted)",
              }}
              aria-label={reviewsLabel}
              aria-pressed={reviewsPanelOpen}
              data-testid="reviews-toggle"
              onClick={onToggleReviewsPanel}
            >
              <GitPullRequest className="h-[15px] w-[15px]" />
              {pendingReviewCount > 0 && (
                <span
                  className="absolute right-px top-px grid h-3.5 min-w-3.5 place-items-center rounded-full px-1 text-[0.5938rem] font-bold leading-none"
                  style={{
                    background: "var(--accent-primary)",
                    color: "var(--text-on-accent)",
                    boxShadow: "0 0 0 2px var(--app-navbar-bg)",
                  }}
                  data-testid="reviews-badge"
                >
                  {pendingReviewCount > 9 ? "9+" : pendingReviewCount}
                </span>
              )}
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="text-xs">
            Toggle reviews <kbd className="ml-1 opacity-70">⌘⇧R</kbd>
          </TooltipContent>
        </Tooltip>

        <ThemeSelector
          open={activeMenu === "theme"}
          onOpenChange={(open) => setActiveMenu(open ? "theme" : null)}
        />
        <FontScaleSelector
          open={activeMenu === "font"}
          onOpenChange={(open) => setActiveMenu(open ? "font" : null)}
        />
      </div>
    </header>
  );
}
