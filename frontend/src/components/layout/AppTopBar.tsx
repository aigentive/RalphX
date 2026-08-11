import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type CSSProperties } from "react";
import { Check, ChevronDown, Inbox, Search } from "lucide-react";
import { useQuery, useQueryClient, type InfiniteData, type QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi, type ChatMessageResponse, type ConversationMessagesPageResponse } from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { notificationsApi } from "@/api/notifications";
import { agentConversationKeys } from "@/components/agents/useProjectAgentConversations";
import { ProjectSelector } from "@/components/projects/ProjectSelector";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { chatKeys } from "@/hooks/useChat";
import { cn } from "@/lib/utils";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { useThemeStore, type FontScale } from "@/stores/themeStore";
import type { AppView } from "@/types/app-view";
import type { ChatConversation } from "@/types/chat-conversation";

import { ThemeSelector } from "./ThemeSelector";

interface AppTopBarProps {
  currentView: AppView;
  attentionCount: number;
  unreadNotificationCount?: number;
  attentionCountStale?: boolean;
  notificationsPanelOpen: boolean;
  onToggleNotificationsPanel: () => void;
  onNewProject?: () => void;
  onProjectSwitchIntent?: (() => void) | undefined;
  showProjectSelector?: boolean;
}

const VIEW_LABELS: Partial<Record<AppView, string>> = {
  agents: "Agents",
  ticketing: "Ticketing",
  github: "GitHub",
  granola: "Granola",
  insights: "Insights",
  extensibility: "Extensibility",
  activity: "Activity",
};

const FONT_SCALE_OPTIONS: Array<{ value: FontScale; label: string }> = [
  { value: "default", label: "100%" },
  { value: "lg", label: "110%" },
  { value: "xl", label: "125%" },
];

let lastDockBadgeCount: number | undefined;

const PROJECT_SELECTOR_VIEWS = new Set<AppView>([
  "ticketing",
  "github",
  "granola",
]);

function viewLabel(view: AppView): string {
  return VIEW_LABELS[view] ?? "Workspace";
}

function breadcrumbItems(
  currentView: AppView,
  projectName: string | null,
  agentConversationTitle: string | null,
): string[] {
  if (currentView === "agents") {
    return ["Workspace", "Agents", agentConversationTitle ?? "New run"];
  }

  if (currentView === "ticketing") {
    return ["Workspace", projectName ?? "Project", "Ticketing"];
  }

  if (currentView === "github") {
    return ["Workspace", projectName ?? "Project", "GitHub"];
  }

  if (currentView === "granola") {
    return ["Workspace", projectName ?? "Project", "Granola"];
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
  queryClient: QueryClient,
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

  queryClient.setQueryData<ChatConversation | null>(
    chatKeys.conversationSummary(conversation.id),
    (oldData) => oldData ? updateConversation(oldData) : oldData,
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

function isCachedConversation(value: unknown, conversationId: string): value is ChatConversation {
  if (!value || typeof value !== "object") {
    return false;
  }
  const candidate = value as Partial<ChatConversation>;
  return (
    candidate.id === conversationId &&
    typeof candidate.contextType === "string" &&
    typeof candidate.contextId === "string"
  );
}

function findConversationInQueryData(
  data: unknown,
  conversationId: string,
): ChatConversation | null {
  if (isCachedConversation(data, conversationId)) {
    return data;
  }

  if (!data || typeof data !== "object") {
    return null;
  }

  const objectData = data as {
    conversation?: unknown;
    conversations?: unknown;
    pages?: unknown;
  };

  if (isCachedConversation(objectData.conversation, conversationId)) {
    return objectData.conversation;
  }

  if (Array.isArray(objectData.conversations)) {
    const conversation = objectData.conversations.find((item) =>
      isCachedConversation(item, conversationId)
    );
    if (conversation) {
      return conversation;
    }
  }

  if (Array.isArray(objectData.pages)) {
    for (const page of objectData.pages) {
      const conversation = findConversationInQueryData(page, conversationId);
      if (conversation) {
        return conversation;
      }
    }
  }

  return null;
}

function findCachedAgentConversation(
  queryClient: QueryClient,
  conversationId: string,
): ChatConversation | null {
  const direct = findConversationInQueryData(
    queryClient.getQueryData(chatKeys.conversation(conversationId)),
    conversationId,
  );
  if (direct) {
    return direct;
  }

  const summary = findConversationInQueryData(
    queryClient.getQueryData(chatKeys.conversationSummary(conversationId)),
    conversationId,
  );
  if (summary) {
    return summary;
  }

  const history = findConversationInQueryData(
    queryClient.getQueryData(chatKeys.conversationHistory(conversationId)),
    conversationId,
  );
  if (history) {
    return history;
  }

  for (const query of queryClient.getQueryCache().findAll({ queryKey: agentConversationKeys.all })) {
    const conversation = findConversationInQueryData(query.state.data, conversationId);
    if (conversation) {
      return conversation;
    }
  }

  return null;
}

function useCachedAgentConversation(
  conversationId: string | null,
  enabled: boolean,
): ChatConversation | null {
  const queryClient = useQueryClient();
  const subscribe = useCallback(
    (onStoreChange: () => void) => queryClient.getQueryCache().subscribe(onStoreChange),
    [queryClient],
  );
  const getSnapshot = useCallback(() => {
    if (!enabled || !conversationId) {
      return null;
    }
    return findCachedAgentConversation(queryClient, conversationId);
  }, [conversationId, enabled, queryClient]);

  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
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
        className={cn(
          "inline-flex h-8 items-center gap-1.5 rounded-[6px] border px-2.5 text-[0.7812rem] font-medium transition-colors duration-150 outline-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]",
          open
            ? "bg-[var(--bg-hover)] border-[var(--border-strong)] text-[var(--text-primary)]"
            : "bg-[var(--bg-elevated)] border-[var(--border-default)] text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:border-[var(--border-strong)] hover:text-[var(--text-primary)]"
        )}
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
  attentionCount,
  unreadNotificationCount = 0,
  attentionCountStale = false,
  notificationsPanelOpen,
  onToggleNotificationsPanel,
  onNewProject,
  onProjectSwitchIntent,
  showProjectSelector = false,
}: AppTopBarProps) {
  const activeProject = useProjectStore(selectActiveProject);
  const selectedAgentConversationId = useAgentSessionStore((s) => s.selectedConversationId);
  const cachedAgentConversation = useCachedAgentConversation(
    selectedAgentConversationId,
    currentView === "agents",
  );
  const selectedAgentConversationSummary = useQuery({
    queryKey: chatKeys.conversationSummary(selectedAgentConversationId ?? ""),
    queryFn: () => chatApi.getConversationSummary(selectedAgentConversationId ?? ""),
    enabled:
      currentView === "agents" &&
      Boolean(selectedAgentConversationId) &&
      !cachedAgentConversation,
    staleTime: 30 * 1000,
  });
  const [activeMenu, setActiveMenu] = useState<"theme" | "font" | null>(null);
  const agentConversation =
    currentView === "agents" && selectedAgentConversationId
      ? cachedAgentConversation ?? selectedAgentConversationSummary.data ?? null
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
  const notificationBadgeCount = attentionCount + unreadNotificationCount;
  const notificationsLabel =
    notificationBadgeCount > 0
      ? `Notifications · ${notificationBadgeCount} total: ${attentionCount} need attention, ${unreadNotificationCount} unread`
      : "Notifications";

  useEffect(() => {
    if (lastDockBadgeCount === notificationBadgeCount) return;

    lastDockBadgeCount = notificationBadgeCount;
    void notificationsApi.setDockBadgeCount(notificationBadgeCount).catch(() => undefined);
  }, [notificationBadgeCount]);

  const shouldShowProjectSelector =
    showProjectSelector && PROJECT_SELECTOR_VIEWS.has(currentView) && Boolean(onNewProject);

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

        {shouldShowProjectSelector && onNewProject && (
          <ProjectSelector
            onNewProject={onNewProject}
            onBeforeProjectChange={onProjectSwitchIntent}
            align="end"
            className="max-w-[240px]"
          />
        )}

        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="relative grid h-8 w-8 place-items-center rounded-[6px] border transition-colors duration-150 outline-none hover:border-[var(--border-default)] hover:bg-[var(--bg-elevated)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
              style={{
                borderColor: "transparent",
                color: notificationsPanelOpen ? "var(--text-primary)" : "var(--text-muted)",
              }}
              aria-label={notificationsLabel}
              aria-pressed={notificationsPanelOpen}
              id="notifications-toggle"
              data-testid="reviews-toggle"
              data-notification-testid="notifications-toggle"
              onClick={onToggleNotificationsPanel}
            >
              <Inbox className="h-[15px] w-[15px]" />
              {notificationBadgeCount > 0 && (
                <span
                  className="absolute right-px top-px grid h-3.5 min-w-3.5 place-items-center rounded-full px-1 text-[10px] font-bold leading-none"
                  style={{
                    backgroundColor: "var(--accent-primary)",
                    color: "var(--text-on-accent)",
                    boxShadow: "0 0 0 2px var(--app-navbar-bg)",
                  }}
                  data-testid="reviews-badge"
                >
                  {notificationBadgeCount > 9 ? "9+" : notificationBadgeCount}
                </span>
              )}
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="text-xs">
            {attentionCountStale ? "⚠ Last known notification count" : "Notifications"} <kbd className="ml-1 opacity-70">⌘⇧R</kbd>
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
