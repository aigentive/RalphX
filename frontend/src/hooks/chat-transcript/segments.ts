import type { StreamingContentBlock } from "@/types/streaming-task";

export interface LiveTranscriptState {
  runId: string | null;
  /** Ordered live slots — preserves the live array's seq/receivedAt ordering. */
  slots: Array<
    | { kind: "text"; blockIndex: number | null; text: string; seq?: number; receivedAt?: number }
    | { kind: "block"; block: StreamingContentBlock }
  >;
  persistedTextByIndex: Map<number, string>;
  /**
   * Persisted text in timeline order. Only consulted for legacy live text that
   * carries no `blockIndex`; indexed slots always resolve through
   * `persistedTextByIndex`, so an interleaved tool block can never shift them.
   */
  persistedTextsInOrder: string[];
  persistedToolIds: Set<string>;
}

export type TranscriptInput =
  | { kind: "persisted"; runId: string | null; blocks: readonly StreamingContentBlock[] }
  | { kind: "chunk"; runId: string | null; blockIndex?: number; text: string; seq?: number; receivedAt?: number; appendToPrevious: boolean }
  | { kind: "thinking"; runId: string | null; blockIndex?: number; text: string; durationMs?: number; isSettled?: boolean; seq?: number; receivedAt?: number; appendToPrevious: boolean }
  | { kind: "segments"; runId: string | null; segments: readonly string[] }
  | { kind: "thinkingSegments"; runId: string | null; segments: readonly string[] }
  /**
   * Legacy cumulative `partial_text` from the active-state cache. It carries no
   * segment identity, so it reconciles against the in-flight (last) text slot.
   */
  | { kind: "partialText"; runId: string | null; text: string }
  | { kind: "tools"; runId: string | null; blocks: readonly StreamingContentBlock[] }
  /**
   * Re-fold already-materialized live blocks. Unlike `chunk`, unindexed text is
   * kept as a distinct slot: those blocks carry no identity, so collapsing them
   * would merge unrelated segments from different turns.
   */
  | { kind: "live"; runId: string | null; blocks: readonly StreamingContentBlock[] };

export function createLiveTranscriptState(runId: string | null = null): LiveTranscriptState {
  return {
    runId,
    slots: [],
    persistedTextByIndex: new Map(),
    persistedTextsInOrder: [],
    persistedToolIds: new Set(),
  };
}

export function applyTranscriptInput(state: LiveTranscriptState, input: TranscriptInput): LiveTranscriptState {
  if (input.runId != null && state.runId != null && input.runId !== state.runId) return state;
  const runId = input.runId ?? state.runId;
  const next: LiveTranscriptState = input.runId != null && input.runId !== state.runId
    ? createLiveTranscriptState(input.runId)
    : {
        ...state,
        runId,
        slots: [...state.slots],
        persistedTextByIndex: new Map(state.persistedTextByIndex),
        persistedTextsInOrder: [...state.persistedTextsInOrder],
        persistedToolIds: new Set(state.persistedToolIds),
      };
  if (input.kind === "persisted") {
    for (const block of input.blocks) {
      if (block.type === "text") {
        next.persistedTextsInOrder.push(block.text);
        if (block.blockIndex != null) next.persistedTextByIndex.set(block.blockIndex, block.text);
      }
      if (block.type === "tool_use") next.persistedToolIds.add(block.toolCall.id);
      if (block.type === "task") next.persistedToolIds.add(block.toolUseId);
    }
    return next;
  }
  if (input.kind === "tools") {
    next.slots.push(...input.blocks.map((block) => ({ kind: "block" as const, block })));
    return next;
  }
  if (input.kind === "live") {
    for (const block of input.blocks) {
      if (block.type === "text") {
        const blockIndex = block.blockIndex ?? null;
        if (blockIndex == null) {
          // No identity to key on. Dedupe only on exact equality, so distinct
          // segments from different turns both survive.
          if (!next.slots.some((slot) => slot.kind === "text" && slot.text === block.text)) {
            next.slots.push(toTextSlot(block, null));
          }
          continue;
        }
        const at = findTextSlot(next, blockIndex);
        if (at < 0) {
          next.slots.push(toTextSlot(block, blockIndex));
          continue;
        }
        // The already-seated slot leads: it is either the durable anchor or an
        // earlier view of this same segment.
        const slot = next.slots[at] as TextSlot;
        next.slots[at] = { ...slot, text: mergeStreamingTextSnapshot(slot.text, block.text) };
        continue;
      }
      if (block.type === "task") {
        const seen = next.slots.some((slot) =>
          slot.kind === "block" && slot.block.type === "task" && slot.block.toolUseId === block.toolUseId
        );
        if (!seen) next.slots.push({ kind: "block", block });
        continue;
      }
      if (block.type === "thinking") {
        const at = next.slots.findIndex((slot) => slot.kind === "block" && slot.block.type === "thinking"
          && slot.block.blockIndex === block.blockIndex);
        if (at >= 0) next.slots[at] = { kind: "block", block };
        else next.slots.push({ kind: "block", block });
        continue;
      }
      const at = next.slots.findIndex((slot) =>
        slot.kind === "block" && slot.block.type === "tool_use" && slot.block.toolCall.id === block.toolCall.id
      );
      if (at >= 0) next.slots[at] = { kind: "block", block };
      else next.slots.push({ kind: "block", block });
    }
    return next;
  }
  if (input.kind === "segments") {
    input.segments.forEach((text, blockIndex) => {
      if (!text) return;
      let at = findTextSlot(next, blockIndex);
      if (at < 0 && !next.slots.some((slot) => slot.kind === "text" && slot.blockIndex != null)) {
        // Nothing indexed yet: a wholly legacy transcript still positions its
        // text slots in segment order.
        at = next.slots.flatMap((slot, index) => (slot.kind === "text" ? [index] : []))[blockIndex] ?? -1;
      }
      if (at < 0) {
        next.slots.push({ kind: "text", blockIndex, text });
        return;
      }
      const slot = next.slots[at] as TextSlot;
      // A segment snapshot names one precise provider block. When it and the
      // seated slot are wholly disjoint, the seated slot is an older projection
      // of a different segment, so the snapshot wins outright.
      const disjoint = slot.blockIndex != null
        && !text.includes(slot.text)
        && !slot.text.includes(text)
        && longestSuffixPrefixOverlap(text, slot.text) === 0
        && longestSuffixPrefixOverlap(slot.text, text) === 0;
      next.slots[at] = {
        ...slot,
        blockIndex,
        text: disjoint ? text : mergeStreamingTextSnapshot(text, slot.text),
      };
    });
    return next;
  }
  if (input.kind === "thinkingSegments") {
    input.segments.forEach((text, blockIndex) => {
      if (!text) return;
      const at = next.slots.findIndex((slot) => slot.kind === "block" && slot.block.type === "thinking"
        && slot.block.blockIndex === blockIndex);
      if (at < 0) {
        next.slots.push({ kind: "block", block: { type: "thinking", text, blockIndex } });
        return;
      }
      const slot = next.slots[at];
      if (slot?.kind !== "block" || slot.block.type !== "thinking") return;
      next.slots[at] = {
        kind: "block",
        block: { ...slot.block, text: mergeStreamingTextSnapshot(text, slot.block.text) },
      };
    });
    return next;
  }
  if (input.kind === "partialText") {
    if (input.text.trim().length === 0) return next;
    const textAt = next.slots.flatMap((slot, index) => (slot.kind === "text" ? [index] : []));
    const first = textAt[0];
    if (first == null) {
      next.slots.push({ kind: "text", blockIndex: null, text: input.text });
      return next;
    }
    const interleaved = textAt.some((index, position) => position > 0
      && next.slots.slice(textAt[position - 1]! + 1, index).some((slot) => slot.kind !== "text"));
    if (!interleaved) {
      mergePartialTextIntoSlot(next, first, input.text);
      return next;
    }
    // Cumulative text spanning interleaved segments: redistribute it across the
    // segments it actually covers instead of dumping the tail on one of them.
    const slotTexts = textAt.map((index) => (next.slots[index] as TextSlot).text);
    const merged = mergeStreamingTextSnapshot(input.text, slotTexts.join(""));
    const starts: number[] = [];
    let searchFrom = 0;
    for (const text of slotTexts) {
      const start = merged.indexOf(text, searchFrom);
      if (start < 0) {
        mergePartialTextIntoSlot(next, first, input.text);
        return next;
      }
      starts.push(start);
      searchFrom = start + text.length;
    }
    textAt.forEach((index, position) => {
      const slot = next.slots[index] as TextSlot;
      const text = merged.slice(starts[position]!, starts[position + 1] ?? merged.length);
      if (text !== slot.text) next.slots[index] = { ...slot, text };
    });
    return next;
  }
  if (input.kind === "thinking") {
    const blockIndex = input.blockIndex;
    const at = next.slots.findIndex((slot) => slot.kind === "block" && slot.block.type === "thinking"
      && slot.block.blockIndex === blockIndex);
    const existing = at >= 0 ? next.slots[at] : null;
    const previous = existing?.kind === "block" && existing.block.type === "thinking" ? existing.block : null;
    const block: StreamingContentBlock = {
      type: "thinking",
      text: previous && input.appendToPrevious ? previous.text + input.text : input.text,
      ...(blockIndex != null ? { blockIndex } : {}),
      ...(input.durationMs != null ? { durationMs: input.durationMs } : {}),
      ...(input.isSettled != null ? { isSettled: input.isSettled } : {}),
      ...(input.seq != null ? { seq: input.seq } : {}),
      ...(input.receivedAt != null ? { receivedAt: input.receivedAt } : {}),
    };
    if (at >= 0) next.slots[at] = { kind: "block", block };
    else next.slots.push({ kind: "block", block });
    return next;
  }
  const blockIndex = input.blockIndex ?? null;
  const at = findTextSlot(next, blockIndex);
  if (at >= 0 && (input.appendToPrevious || blockIndex == null)) {
    const slot = next.slots[at] as TextSlot;
    next.slots[at] = {
      ...slot,
      text: slot.text + input.text,
      ...(input.seq != null ? { seq: Math.max(slot.seq ?? input.seq, input.seq) } : {}),
    };
    return next;
  }
  if (at >= 0) {
    // Same identity, non-append chunk: the chunk restates the segment.
    next.slots[at] = { ...(next.slots[at] as TextSlot), text: input.text };
    return next;
  }
  next.slots.push({
    kind: "text",
    blockIndex,
    text: input.text,
    ...(input.seq != null ? { seq: input.seq } : {}),
    ...(input.receivedAt != null ? { receivedAt: input.receivedAt } : {}),
  });
  return next;
}

type TextSlot = Extract<LiveTranscriptState["slots"][number], { kind: "text" }>;

/** Locate the slot owning `blockIndex` anywhere in the transcript, not just at the tail. */
function findTextSlot(state: LiveTranscriptState, blockIndex: number | null): number {
  return state.slots.findIndex((slot) => slot.kind === "text" && slot.blockIndex === blockIndex);
}

function mergePartialTextIntoSlot(state: LiveTranscriptState, index: number, partialText: string): void {
  const slot = state.slots[index] as TextSlot;
  state.slots[index] = { ...slot, text: mergeStreamingTextSnapshot(partialText, slot.text) };
}

function toTextSlot(
  block: Extract<StreamingContentBlock, { type: "text" }>,
  blockIndex: number | null,
): TextSlot {
  return {
    kind: "text",
    blockIndex,
    text: block.text,
    ...(block.seq != null ? { seq: block.seq } : {}),
    ...(block.receivedAt != null ? { receivedAt: block.receivedAt } : {}),
  };
}

/**
 * Keep the previous array identity when a recomputation changed nothing.
 * Recovery polls on an interval, so returning a fresh-but-equal array would
 * re-render the whole transcript on every poll.
 */
export function preserveBlocksIfUnchanged(
  previous: readonly StreamingContentBlock[],
  next: StreamingContentBlock[],
): StreamingContentBlock[] {
  if (previous.length !== next.length) return next;
  const unchanged = previous.every((block, index) => {
    const candidate = next[index];
    return candidate != null
      && (block === candidate
        || (block.type === "text" && candidate.type === "text"
          && block.text === candidate.text
          && block.blockIndex === candidate.blockIndex));
  });
  return unchanged ? (previous as StreamingContentBlock[]) : next;
}

/**
 * Materialize slots as live blocks with their full text. Used by recovery,
 * which seeds the live cache rather than rendering a tail, so nothing is
 * subtracted here.
 */
export function renderTranscriptSlots(state: LiveTranscriptState): StreamingContentBlock[] {
  return state.slots.map((slot) => (slot.kind === "block" ? slot.block : {
    type: "text" as const,
    text: slot.text,
    ...(slot.blockIndex != null ? { blockIndex: slot.blockIndex } : {}),
    ...(slot.seq != null ? { seq: slot.seq } : {}),
    ...(slot.receivedAt != null ? { receivedAt: slot.receivedAt } : {}),
  }));
}

/**
 * Render the supplementary live tail. Persisted rows are the authority and are
 * already on screen, so each text slot emits only what persistence does not yet
 * cover. Slot order is preserved verbatim — tool/task interleaving is owned by
 * `seq`/`receivedAt`, never by `blockIndex`.
 */
export function renderTranscriptBlocks(state: LiveTranscriptState): StreamingContentBlock[] {
  let textOrdinal = 0;

  return state.slots.flatMap((slot): StreamingContentBlock[] => {
    if (slot.kind === "block") {
      const id = slot.block.type === "tool_use"
        ? slot.block.toolCall.id
        : slot.block.type === "task" ? slot.block.toolUseId : undefined;
      return id != null && state.persistedToolIds.has(id) ? [] : [slot.block];
    }

    const live: StreamingContentBlock = {
      type: "text",
      text: slot.text,
      ...(slot.blockIndex != null ? { blockIndex: slot.blockIndex } : {}),
      ...(slot.seq != null ? { seq: slot.seq } : {}),
      ...(slot.receivedAt != null ? { receivedAt: slot.receivedAt } : {}),
    };

    // Indexed slots resolve by identity. Unindexed legacy slots have no identity
    // to resolve with, so they fall back to ordinal position as before.
    const persisted = slot.blockIndex != null
      ? state.persistedTextByIndex.get(slot.blockIndex)
      : state.persistedTextsInOrder[textOrdinal];
    textOrdinal += 1;

    if (persisted == null) return [live];
    if (slot.text.startsWith(persisted)) {
      const text = slot.text.slice(persisted.length);
      return text ? [{ ...live, text }] : [];
    }
    if (persisted.startsWith(slot.text)) return [];
    // Divergent: never hide live content behind a mismatched persisted row.
    return [live];
  });
}

export function mergeStreamingTextSnapshot(snapshotText: string, liveText: string): string {
  if (liveText.length === 0) {
    return snapshotText;
  }
  if (snapshotText === liveText) {
    return liveText;
  }
  if (snapshotText.startsWith(liveText) || snapshotText.endsWith(liveText)) {
    return snapshotText;
  }
  if (liveText.startsWith(snapshotText) || liveText.endsWith(snapshotText)) {
    return liveText;
  }

  const snapshotThenLiveOverlap = longestSuffixPrefixOverlap(snapshotText, liveText);
  const liveThenSnapshotOverlap = longestSuffixPrefixOverlap(liveText, snapshotText);
  if (liveThenSnapshotOverlap > snapshotThenLiveOverlap) {
    return liveText + snapshotText.slice(liveThenSnapshotOverlap);
  }
  return snapshotText + liveText.slice(snapshotThenLiveOverlap);
}

function longestSuffixPrefixOverlap(left: string, right: string): number {
  const maxLength = Math.min(left.length, right.length);
  for (let length = maxLength; length > 0; length -= 1) {
    if (left.endsWith(right.slice(0, length))) {
      return length;
    }
  }
  return 0;
}
