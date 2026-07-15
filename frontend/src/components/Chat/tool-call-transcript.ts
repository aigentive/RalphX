import type { ToolCall } from "./tool-widgets/shared.constants";
import { normalizeDelegationTranscriptPayload } from "./delegation-tool-calls";

type TranscriptContentBlock = {
  type: string;
  name?: string;
  arguments?: unknown;
  result?: unknown;
  error?: string;
};

interface NormalizeToolCallTranscriptPayloadArgs<
  TContentBlock extends TranscriptContentBlock,
  TToolCall extends ToolCall,
> {
  contentBlocks?: TContentBlock[] | null | undefined;
  toolCalls?: TToolCall[] | null | undefined;
}

export function normalizeToolCallTranscriptPayload<
  TContentBlock extends TranscriptContentBlock,
  TToolCall extends ToolCall,
>({
  contentBlocks,
  toolCalls,
}: NormalizeToolCallTranscriptPayloadArgs<TContentBlock, TToolCall>): {
  contentBlocks: TContentBlock[];
  toolCalls: TToolCall[];
} {
  return normalizeDelegationTranscriptPayload<TContentBlock, TToolCall>({
    contentBlocks,
    toolCalls,
  });
}
