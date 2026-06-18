import { useEffect, useRef } from "react";
import { FileText, Folder, Hash, Link2, ScrollText, Wrench } from "lucide-react";

import { cn } from "@/lib/utils";

export type AgentComposerMenuItemKind =
  | "path"
  | "plan"
  | "skill"
  | "slash-command"
  | "integration";

export interface AgentComposerMenuItem {
  id: string;
  kind: AgentComposerMenuItemKind;
  label: string;
  description?: string;
  detail?: string;
  sourceLabel?: string;
  disabled?: boolean;
}

interface AgentComposerCommandMenuProps {
  items: AgentComposerMenuItem[];
  activeIndex: number;
  onActiveIndexChange: (index: number) => void;
  onSelect: (item: AgentComposerMenuItem) => void;
  isLoading?: boolean;
  emptyLabel: string;
}

const MAX_VISIBLE_MENU_ITEMS = 5;
const MENU_ITEM_MIN_HEIGHT_PX = 52;

export function AgentComposerCommandMenu({
  items,
  activeIndex,
  onActiveIndexChange,
  onSelect,
  isLoading = false,
  emptyLabel,
}: AgentComposerCommandMenuProps) {
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    const activeItem = itemRefs.current[activeIndex];
    if (typeof activeItem?.scrollIntoView !== "function") {
      return;
    }
    activeItem.scrollIntoView({
      block: "nearest",
    });
  }, [activeIndex]);

  return (
    <div
      role="listbox"
      aria-label="Composer suggestions"
      className="mx-3 mt-3 overflow-hidden rounded-lg border p-1 shadow-lg"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
      }}
      data-testid="agent-composer-command-menu"
    >
      {items.length > 0 ? (
        <div
          className="overflow-y-auto overscroll-contain pr-1"
          style={{
            maxHeight: `${MAX_VISIBLE_MENU_ITEMS * MENU_ITEM_MIN_HEIGHT_PX}px`,
          }}
          data-testid="agent-composer-command-menu-scroll"
        >
          {items.map((item, index) => {
            const active = index === activeIndex;
            const Icon =
              item.kind === "path"
                ? item.detail === "directory"
                  ? Folder
                  : FileText
                : item.kind === "plan"
                  ? ScrollText
                : item.kind === "skill"
                  ? Wrench
                  : item.kind === "integration"
                    ? Link2
                    : Hash;
            return (
              <button
                key={item.id}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                type="button"
                role="option"
                aria-selected={active}
                disabled={item.disabled}
                className={cn(
                  "flex min-h-[52px] w-full items-start gap-2 rounded-lg px-2.5 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                  active ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--bg-hover)]",
                )}
                onMouseEnter={() => onActiveIndexChange(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => onSelect(item)}
                data-testid={`agent-composer-menu-item-${item.id}`}
              >
                <Icon className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-secondary)]" />
                <span className="min-w-0 flex-1">
                  <span className="flex min-w-0 items-center gap-2">
                    <span className="truncate text-[0.8125rem] font-medium text-[var(--text-primary)]">
                      {item.label}
                    </span>
                    {item.sourceLabel ? (
                      <span className="shrink-0 rounded-md border px-1.5 py-0.5 text-[0.625rem] font-medium uppercase text-[var(--text-muted)]">
                        {item.sourceLabel}
                      </span>
                    ) : null}
                  </span>
                  {item.description ? (
                    <span className="mt-0.5 block truncate text-[0.6875rem] text-[var(--text-muted)]">
                      {item.description}
                    </span>
                  ) : null}
                </span>
              </button>
            );
          })}
        </div>
      ) : (
        <div className="px-3 py-2 text-[0.75rem] text-[var(--text-muted)]">
          {isLoading ? "Loading..." : emptyLabel}
        </div>
      )}
    </div>
  );
}
