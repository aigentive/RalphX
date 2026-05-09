import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { chatApi } from "@/api/chat";
import type { ToolCall } from "./tool-widgets/shared.constants";

function getDetailKey(
  id: string,
  ref: ToolCall["detailRef"],
): string {
  return [
    id,
    ref?.conversationId ?? "",
    ref?.messageId ?? "",
    ref?.toolCallId ?? "",
    ref?.contentBlockIndex ?? "",
  ].join(":");
}

function getDetailErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Full result failed to load.";
}

export function useLazyToolCallDetail(toolCall: ToolCall) {
  const [fullToolCall, setFullToolCall] = useState<ToolCall | null>(null);
  const [isLoadingDetail, setIsLoadingDetail] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const isMountedRef = useRef(true);
  const requestedDetailKeyRef = useRef<string | null>(null);
  const detailRef = toolCall.detailRef;
  const detailKey = useMemo(
    () => getDetailKey(toolCall.id, detailRef),
    [detailRef, toolCall.id],
  );
  const displayToolCall = fullToolCall ?? toolCall;

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    requestedDetailKeyRef.current = null;
    setFullToolCall(null);
    setIsLoadingDetail(false);
    setDetailError(null);
  }, [detailKey]);

  const loadDetail = useCallback(async () => {
    if (
      !toolCall.resultPreviewTruncated
      || !detailRef
      || fullToolCall
      || isLoadingDetail
      || requestedDetailKeyRef.current === detailKey
    ) {
      return;
    }

    requestedDetailKeyRef.current = detailKey;
    setIsLoadingDetail(true);
    setDetailError(null);

    try {
      const response = await chatApi.getAgentMessageToolCallDetail(detailRef);
      if (!isMountedRef.current) return;
      if (response?.toolCall) {
        setFullToolCall(response.toolCall);
      } else {
        setDetailError("Full result is unavailable.");
      }
    } catch (error: unknown) {
      if (!isMountedRef.current) return;
      setDetailError(getDetailErrorMessage(error));
    } finally {
      if (isMountedRef.current) {
        setIsLoadingDetail(false);
      }
    }
  }, [
    detailKey,
    detailRef,
    fullToolCall,
    isLoadingDetail,
    toolCall.resultPreviewTruncated,
  ]);

  return {
    detailError,
    displayToolCall,
    fullToolCall,
    isLoadingDetail,
    loadDetail,
  };
}
