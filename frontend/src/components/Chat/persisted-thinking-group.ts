import { isProviderRole } from "@/lib/chat/provider-role";

export interface PersistedGroupSurface {
  role: string;
  parentMessageId?: string | null | undefined;
  sender?: string | null | undefined;
  providerHarness?: string | null | undefined;
  providerSessionId?: string | null | undefined;
  upstreamProvider?: string | null | undefined;
  providerProfile?: string | null | undefined;
  timelineSequence?: number | null | undefined;
  contentBlocks?: readonly { type: string; text?: string | undefined }[] | null | undefined;
}

export function samePersistedGroupSurface(
  left: PersistedGroupSurface,
  right: PersistedGroupSurface,
): boolean {
  return left.role === right.role
    && (left.sender ?? null) === (right.sender ?? null)
    && (left.providerHarness ?? null) === (right.providerHarness ?? null)
    && (left.providerSessionId ?? null) === (right.providerSessionId ?? null)
    && (left.upstreamProvider ?? null) === (right.upstreamProvider ?? null)
    && (left.providerProfile ?? null) === (right.providerProfile ?? null);
}

function isPersistedThinkingMessage(item: PersistedGroupSurface): boolean {
  const blocks = item.contentBlocks;
  return isProviderRole(item.role)
    && item.timelineSequence != null
    && blocks?.length === 1
    && blocks[0]?.type === "thinking"
    && Boolean(blocks[0].text?.trim());
}

function hasSameParent(first: PersistedGroupSurface, next: PersistedGroupSurface): boolean {
  if (!first.parentMessageId && !next.parentMessageId) return true;
  return first.parentMessageId != null && first.parentMessageId === next.parentMessageId;
}

export function collectPersistedThinkingRun<T extends PersistedGroupSurface>(
  items: readonly T[],
  startIndex: number,
): T[] | null {
  const first = items[startIndex];
  if (!first || !isPersistedThinkingMessage(first)) return null;

  const run = [first];
  let previous = first;
  for (let index = startIndex + 1; index < items.length; index += 1) {
    const next = items[index];
    if (
      !next
      || !isPersistedThinkingMessage(next)
      || !samePersistedGroupSurface(first, next)
      || !hasSameParent(first, next)
      || previous.timelineSequence == null
      || next.timelineSequence == null
      || next.timelineSequence !== previous.timelineSequence + 1
    ) {
      break;
    }
    run.push(next);
    previous = next;
  }

  return run.length >= 2 ? run : null;
}
