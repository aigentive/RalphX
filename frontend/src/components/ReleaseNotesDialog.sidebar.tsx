import { memo, useEffect, useRef } from "react";
import { Loader2 } from "lucide-react";

import type { SidebarItem } from "./ReleaseNotesDialog.sidebar-items";

export const VersionSidebar = memo(function VersionSidebar({
  items,
  activeVersion,
  loading,
  onClick,
}: {
  items: SidebarItem[];
  activeVersion: string | null;
  loading: boolean;
  onClick: (version: string) => void;
}) {
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    activeRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [activeVersion]);

  if (loading) {
    return (
      <div
        className="flex w-52 shrink-0 items-center justify-center border-l"
        style={{
          borderColor: "var(--border-subtle)",
          backgroundColor: "var(--bg-surface)",
        }}
      >
        <Loader2
          className="h-4 w-4 animate-spin"
          style={{ color: "var(--text-muted)" }}
        />
      </div>
    );
  }

  return (
    <div
      className="w-52 shrink-0 overflow-y-auto border-l"
      style={{
        borderColor: "var(--border-subtle)",
        backgroundColor: "var(--bg-surface)",
      }}
    >
      <div className="py-2">
        {items.map((item, i) => {
          if (item.kind === "header") {
            return (
              <div
                key={`h-${item.label}`}
                className="px-4 pb-1 text-[0.6875rem] font-semibold uppercase tracking-wider"
                style={{
                  color: "var(--text-muted)",
                  paddingTop: i === 0 ? "4px" : "12px",
                }}
              >
                {item.label}
              </div>
            );
          }

          const isActive = item.version === activeVersion;
          return (
            <button
              key={item.version}
              ref={isActive ? activeRef : undefined}
              type="button"
              className="flex w-full items-center justify-between gap-2 rounded-none px-4 py-1.5 text-left transition-colors"
              style={{
                color: isActive
                  ? "var(--accent-primary)"
                  : "var(--text-secondary)",
                backgroundColor: isActive
                  ? "var(--bg-elevated)"
                  : "transparent",
                borderLeft: isActive
                  ? "2px solid var(--accent-primary)"
                  : "2px solid transparent",
              }}
              onClick={() => onClick(item.version)}
            >
              <span className="truncate text-[0.8125rem] font-medium">
                v{item.version}
              </span>
              {item.isCurrent ? (
                <span
                  className="shrink-0 text-[0.6875rem] font-semibold"
                  style={{ color: "var(--accent-primary)" }}
                >
                  current
                </span>
              ) : item.date ? (
                <span
                  className="shrink-0 text-[0.6875rem]"
                  style={{ color: "var(--text-muted)" }}
                >
                  {item.date}
                </span>
              ) : null}
            </button>
          );
        })}
      </div>
    </div>
  );
});
