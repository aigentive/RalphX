export interface ThinkingSegmentLifecycle {
  isSettled?: boolean | undefined;
  durationMs?: number | undefined;
  estimatedTokens?: number | undefined;
}

export interface ThinkingGroupAggregate {
  isSettled: boolean;
  segmentCount: number;
  totalDurationMs?: number | undefined;
  estimatedTokens?: number | undefined;
}

export function aggregateThinkingSegments(
  segments: readonly ThinkingSegmentLifecycle[],
  settledDefault: boolean,
): ThinkingGroupAggregate {
  const isSettled = segments.every((segment) => segment.isSettled ?? settledDefault);
  const durations = segments
    .map((segment) => segment.durationMs)
    .filter((duration): duration is number => duration != null);
  const latestTokenSegment = [...segments].reverse().find((segment) => segment.estimatedTokens != null);

  return {
    isSettled,
    segmentCount: segments.length,
    ...(durations.length > 0 ? { totalDurationMs: durations.reduce((total, duration) => total + duration, 0) } : {}),
    ...(!isSettled && latestTokenSegment?.estimatedTokens != null
      ? { estimatedTokens: latestTokenSegment.estimatedTokens }
      : {}),
  };
}

export function joinThinkingSegmentTexts(texts: readonly (string | undefined)[]): string {
  return texts
    .map((text) => text?.trim())
    .filter((text): text is string => Boolean(text))
    .join("\n\n···\n\n");
}
