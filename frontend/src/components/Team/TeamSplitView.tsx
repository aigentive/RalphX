/**
 * TeamSplitView — Top-level team split view component
 *
 * Full-height container rendering TeamSplitHeader + TeamSplitGrid.
 * Reads team state from teamStore, sets contextKey on the splitPaneStore.
 */

import React, { useEffect } from "react";
import { useSplitPaneStore } from "@/stores/splitPaneStore";
import { TeamSplitHeader } from "./TeamSplitHeader";
import { TeamSplitGrid } from "./TeamSplitGrid";

interface TeamSplitViewProps {
  contextKey?: string | undefined;
}

export const TeamSplitView = React.memo(function TeamSplitView({
  contextKey: contextKeyProp,
}: TeamSplitViewProps) {
  const storeContextKey = useSplitPaneStore((s) => s.contextKey);
  const setContextKey = useSplitPaneStore((s) => s.setContextKey);

  // Use prop if provided, otherwise fall back to store
  const contextKey = contextKeyProp ?? storeContextKey ?? "";

  // Sync context key from prop to store
  useEffect(() => {
    if (contextKeyProp && contextKeyProp !== storeContextKey) {
      setContextKey(contextKeyProp);
    }
  }, [contextKeyProp, storeContextKey, setContextKey]);

  if (!contextKey) {
    return (
      <div
        className="flex items-center justify-center h-full"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        <span className="text-[0.8125rem]" style={{ color: "var(--text-muted)" }}>
          No active team
        </span>
      </div>
    );
  }

  return (
    <div
      className="flex flex-col h-full"
      style={{ backgroundColor: "var(--bg-base)" }}
    >
      <TeamSplitHeader contextKey={contextKey} />
      <TeamSplitGrid />
    </div>
  );
});
