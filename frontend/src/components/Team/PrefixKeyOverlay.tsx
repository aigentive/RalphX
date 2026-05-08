/**
 * PrefixKeyOverlay — Small keyboard indicator toast for tmux-style navigation
 *
 * Fixed position, bottom-right corner. Shows when Ctrl+B prefix is active.
 * Glass-morphism styling with auto-fade-in animation.
 */

import { useSplitPaneStore } from "@/stores/splitPaneStore";

export function PrefixKeyOverlay() {
  const isActive = useSplitPaneStore((s) => s.isPrefixKeyActive);

  if (!isActive) return null;

  return (
    <div
      className="fixed bottom-6 right-6 z-50 animate-in fade-in duration-150"
      style={{
        background: "var(--bg-surface)",
        backdropFilter: "blur(12px)",
        WebkitBackdropFilter: "blur(12px)",
        border: "1px solid var(--overlay-moderate)",
        borderRadius: "8px",
        padding: "8px 14px",
      }}
    >
      <div className="flex items-center gap-2 text-xs text-text-secondary">
        <kbd
          className="rounded px-1.5 py-0.5 font-mono text-[0.6875rem] text-text-primary"
          style={{
            background: "var(--overlay-weak)",
            border: "1px solid var(--overlay-moderate)",
          }}
        >
          Ctrl+B
        </kbd>
        <span>active — press arrow or 1-5</span>
      </div>
    </div>
  );
}
