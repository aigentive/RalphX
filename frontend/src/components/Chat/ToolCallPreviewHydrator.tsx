import type { KeyboardEvent, ReactNode } from "react";
import { useCallback } from "react";

import type { ToolCall } from "./tool-widgets/shared.constants";
import { useLazyToolCallDetail } from "./useLazyToolCallDetail";

interface ToolCallPreviewHydratorProps {
  toolCall: ToolCall;
  children: (toolCall: ToolCall) => ReactNode;
}

export function ToolCallPreviewHydrator({
  toolCall,
  children,
}: ToolCallPreviewHydratorProps) {
  const { displayToolCall, loadDetail } = useLazyToolCallDetail(toolCall);

  const loadOnInteraction = useCallback(() => {
    void loadDetail();
  }, [loadDetail]);

  const loadOnKeyboardExpansion = useCallback((event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter" || event.key === " ") {
      void loadDetail();
    }
  }, [loadDetail]);

  return (
    <div
      data-testid="tool-call-preview-hydrator"
      onClickCapture={loadOnInteraction}
      onKeyDownCapture={loadOnKeyboardExpansion}
    >
      {children(displayToolCall)}
    </div>
  );
}
