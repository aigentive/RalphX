/**
 * Shared RalphX association rail.
 *
 * Extracted verbatim from `ticketing/TicketDetailSheet.tsx` so PR-detail surfaces
 * can reuse the same association panel without duplicating it. Behavior and the
 * prop contract are unchanged from the original inline implementation.
 */
import { useMemo, useState } from "react";
import { GitBranch, GitPullRequestArrow } from "lucide-react";

import type {
  TicketAssociationItem,
  TicketAssociations,
  TicketDeepLink,
  TicketDetail,
  TicketSummary,
} from "@/api/ticketing";
import { Button } from "@/components/ui/button";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { ticketCanonicalBranchName } from "@/components/ticketing/ticketing-utils";

const ASSOCIATION_GROUPS: Array<{
  key: keyof Omit<TicketAssociations, "fetchedAt">;
  label: string;
}> = [
  { key: "tasks", label: "Tasks" },
  { key: "conversations", label: "Conversations" },
  { key: "sessions", label: "Sessions" },
  { key: "proposals", label: "Proposals" },
  { key: "pullRequests", label: "Pull Requests" },
  { key: "specs", label: "Specs" },
  { key: "checks", label: "Checks" },
  { key: "qa", label: "QA" },
];

function AssociationCard({
  item,
  onNavigate,
  leadingIcon,
}: {
  item: TicketAssociationItem;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  leadingIcon?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      className="w-full rounded-md px-3 py-2 text-left hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: "var(--text-primary)",
      }}
      onClick={() => onNavigate?.(item.deepLink)}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-1.5">
          {leadingIcon}
          <span className="truncate text-sm font-medium">{item.title}</span>
        </span>
        {item.active && (
          <span
            className="shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium text-[var(--text-primary)]"
            style={{
              backgroundColor: "var(--accent-muted)",
              borderColor: "var(--accent-border)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            Active
          </span>
        )}
      </div>
      {(item.status || item.subtitle) && (
        <p className="mt-1 truncate text-xs text-[var(--text-muted)]">
          {item.status ?? item.subtitle}
        </p>
      )}
    </button>
  );
}

/**
 * Inline picker that binds an existing project conversation to this ticket.
 * The toggle is local state so the first click paints synchronously without any
 * async work; selecting a row calls back and closes the picker.
 */
function BindConversationControl({
  conversations,
  onBindConversation,
  isBindPending = false,
  bindError,
}: {
  conversations: { id: string; title: string | null }[];
  onBindConversation?: ((conversationId: string) => void) | undefined;
  isBindPending?: boolean | undefined;
  bindError?: string | null | undefined;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return conversations;
    }
    return conversations.filter((conversation) =>
      (conversation.title ?? "").toLowerCase().includes(needle),
    );
  }, [conversations, query]);

  function handleBind(conversationId: string) {
    onBindConversation?.(conversationId);
    setOpen(false);
    setQuery("");
  }

  return (
    <div className="mt-2">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-full justify-center"
        disabled={!onBindConversation || isBindPending}
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
      >
        {isBindPending ? "Binding..." : "Bind existing conversation"}
      </Button>
      {open && (
        <div
          className="mt-2 rounded-md p-2"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          <input
            type="text"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search conversations"
            aria-label="Search conversations"
            className="h-8 w-full rounded-md px-2 text-xs outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            style={{
              backgroundColor: "var(--bg-elevated)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-primary)",
            }}
          />
          <div className="mt-2 max-h-48 space-y-1 overflow-auto">
            {filtered.length === 0 ? (
              <p className="px-1 py-2 text-xs text-[var(--text-muted)]">
                No conversations to bind.
              </p>
            ) : (
              filtered.map((conversation) => (
                <button
                  key={conversation.id}
                  type="button"
                  className="w-full truncate rounded-md px-2 py-1.5 text-left text-xs hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    borderColor: "var(--border-subtle)",
                    borderStyle: "solid",
                    borderWidth: "1px",
                    color: "var(--text-primary)",
                  }}
                  onClick={() => handleBind(conversation.id)}
                >
                  {conversation.title || "Untitled agent"}
                </button>
              ))
            )}
          </div>
        </div>
      )}
      {bindError && (
        <p className="mt-2 text-xs text-[var(--status-error)]" role="alert">
          {bindError}
        </p>
      )}
    </div>
  );
}

export function RalphxAssociationPanel({
  ticket,
  associations,
  isLoading,
  isStartWorkPending = false,
  startWorkError,
  showConversationBinding = true,
  bindableConversations,
  onBindConversation,
  isBindPending,
  bindError,
  onNavigate,
  onStartWork,
}: {
  ticket: TicketDetail | TicketSummary | null;
  associations: TicketAssociations | undefined;
  isLoading: boolean;
  isStartWorkPending?: boolean | undefined;
  startWorkError?: string | null | undefined;
  showConversationBinding?: boolean | undefined;
  bindableConversations?: { id: string; title: string | null }[] | undefined;
  onBindConversation?: ((conversationId: string) => void) | undefined;
  isBindPending?: boolean | undefined;
  bindError?: string | null | undefined;
  onNavigate?: ((deepLink: TicketDeepLink) => void) | undefined;
  onStartWork?: (() => void) | undefined;
}) {
  const ticketBranchName = ticket ? ticketCanonicalBranchName(ticket.ref) : null;
  const hasTicketPr = Boolean(ticket?.openPrNumber);
  const hasTicketGitMetadata = Boolean(ticketBranchName || hasTicketPr);
  const activeCount = ASSOCIATION_GROUPS.reduce((count, group) => {
    return count + (associations?.[group.key].filter((item) => item.active).length ?? 0);
  }, 0);
  const totalCount = ASSOCIATION_GROUPS.reduce((count, group) => {
    return count + (associations?.[group.key].length ?? 0);
  }, 0);

  return (
    <aside
      className="flex min-h-0 flex-col p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderLeftColor: "var(--border-subtle)",
        borderLeftStyle: "solid",
        borderLeftWidth: "1px",
      }}
    >
      <div className="flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-[var(--text-primary)]">RalphX Work</h3>
        {activeCount > 0 && (
          <span
            className="rounded-full px-2 py-0.5 text-xs font-medium text-[var(--text-primary)]"
            style={{
              backgroundColor: "var(--accent-muted)",
              borderColor: "var(--accent-border)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            ● Active
          </span>
        )}
      </div>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="mt-3 w-full justify-center"
        disabled={!onStartWork || isStartWorkPending}
        onClick={onStartWork}
      >
        {isStartWorkPending
          ? "Starting..."
          : "Start conversation"}
      </Button>
      {startWorkError && (
        <p className="mt-2 text-xs text-[var(--status-error)]" role="alert">
          {startWorkError}
        </p>
      )}
      {showConversationBinding && (
        <>
          <BindConversationControl
            conversations={bindableConversations ?? []}
            onBindConversation={onBindConversation}
            isBindPending={isBindPending}
            bindError={bindError}
          />
        </>
      )}
      {isLoading ? (
        <p className="mt-4 text-sm text-[var(--text-muted)]">Loading associations</p>
      ) : (
        <div className="mt-4 min-h-0 space-y-4 overflow-auto">
          {hasTicketGitMetadata && (
            <section>
              <h4 className="mb-2 text-[11px] font-semibold uppercase text-[var(--text-muted)]">
                Ticket Git
              </h4>
              <div
                className="space-y-2 rounded-md p-3"
                style={{
                  backgroundColor: "var(--bg-elevated)",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                }}
              >
                {ticketBranchName && (
                  <div>
                    <p className="text-[11px] font-medium uppercase text-[var(--text-muted)]">
                      Branch
                    </p>
                    <p className="mt-1 break-all font-mono text-xs text-[var(--text-primary)]">
                      {ticketBranchName}
                    </p>
                  </div>
                )}
                {hasTicketPr && (
                  <div>
                    <p className="text-[11px] font-medium uppercase text-[var(--text-muted)]">
                      Pull request
                    </p>
                    {ticket?.openPrUrl ? (
                      <button
                        type="button"
                        className="mt-1 inline-flex items-center gap-1.5 rounded text-xs font-medium text-[var(--status-info)] hover:underline focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                        onClick={() => void openExternalTicketUrl(ticket.openPrUrl ?? "")}
                      >
                        <GitPullRequestArrow className="h-3.5 w-3.5" aria-hidden="true" />
                        PR #{ticket.openPrNumber}
                        {ticket.openPrStatus ? ` (${ticket.openPrStatus})` : ""}
                      </button>
                    ) : (
                      <p className="mt-1 text-xs text-[var(--text-primary)]">
                        PR #{ticket?.openPrNumber}
                        {ticket?.openPrStatus ? ` (${ticket.openPrStatus})` : ""}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </section>
          )}
          {totalCount === 0 && (
            <p className="text-sm text-[var(--text-muted)]">
              No RalphX links yet. Start a conversation with this ticket attached.
            </p>
          )}
          {ASSOCIATION_GROUPS.map((group) => {
            const items = associations?.[group.key] ?? [];
            if (items.length === 0) {
              return null;
            }
            return (
              <section key={group.key}>
                <h4 className="mb-2 text-[11px] font-semibold uppercase text-[var(--text-muted)]">
                  {group.label} ({items.length})
                </h4>
                <div className="space-y-2">
                  {items.map((item) => {
                    // In the Pull Requests bucket, distinguish a real PR from a
                    // branch-only workspace at a glance (the backend marks
                    // branch-only items with status "branch").
                    const prIcon =
                      group.key === "pullRequests"
                        ? item.status === "branch"
                          ? (
                              <span
                                role="img"
                                aria-label="Branch only (no pull request yet)"
                                title="Branch only (no pull request yet)"
                                className="inline-flex shrink-0 text-[var(--text-muted)]"
                              >
                                <GitBranch className="h-3.5 w-3.5" aria-hidden="true" />
                              </span>
                            )
                          : (
                              <span
                                role="img"
                                aria-label={
                                  item.active
                                    ? "Open pull request"
                                    : "Pull request (merged or closed)"
                                }
                                title={
                                  item.active
                                    ? "Open pull request"
                                    : "Pull request (merged or closed)"
                                }
                                className={
                                  item.active
                                    ? "inline-flex shrink-0 text-[var(--status-success)]"
                                    : "inline-flex shrink-0 text-[var(--text-muted)]"
                                }
                              >
                                <GitPullRequestArrow className="h-3.5 w-3.5" aria-hidden="true" />
                              </span>
                            )
                        : undefined;
                    return (
                      <AssociationCard
                        key={`${group.key}:${item.id}`}
                        item={item}
                        onNavigate={onNavigate}
                        {...(prIcon !== undefined && { leadingIcon: prIcon })}
                      />
                    );
                  })}
                </div>
              </section>
            );
          })}
        </div>
      )}
    </aside>
  );
}
