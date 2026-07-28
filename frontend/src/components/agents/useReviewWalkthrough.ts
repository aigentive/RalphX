import { useCallback, useEffect, useState } from "react";

interface UseReviewWalkthroughOptions {
  findingIds: string[];
  onCurrentFindingChange?: ((findingId: string | null) => void) | undefined;
}

export function useReviewWalkthrough({
  findingIds,
  onCurrentFindingChange,
}: UseReviewWalkthroughOptions) {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [isComplete, setIsComplete] = useState(false);
  const [reviewedIds, setReviewedIds] = useState<Set<string>>(() => new Set());
  const findingCount = findingIds.length;
  const findingSignature = findingIds.join("\u0000");

  useEffect(() => {
    setCurrentIndex(0);
    setIsComplete(false);
    setReviewedIds(new Set());
  }, [findingSignature]);

  const currentFindingId = isComplete ? null : (findingIds[currentIndex] ?? null);

  useEffect(() => {
    onCurrentFindingChange?.(currentFindingId);
  }, [currentFindingId, onCurrentFindingChange]);

  const goTo = useCallback(
    (index: number) => {
      if (findingCount === 0) return;
      setIsComplete(false);
      setCurrentIndex(Math.max(0, Math.min(findingCount - 1, index)));
    },
    [findingCount],
  );

  const next = useCallback(() => {
    if (findingCount === 0) return;
    setCurrentIndex((index) => {
      if (index === findingCount - 1) {
        setIsComplete(true);
        return index;
      }
      return index + 1;
    });
  }, [findingCount]);

  // Stepping back off the completion screen returns to the last finding with
  // reviewed marks intact; only `restart` is allowed to clear them.
  const previous = useCallback(() => {
    if (isComplete) {
      setIsComplete(false);
      return;
    }
    setCurrentIndex((index) => Math.max(0, index - 1));
  }, [isComplete]);

  const toggleReviewed = useCallback(() => {
    const findingId = findingIds[currentIndex];
    if (findingId === undefined) return;

    if (reviewedIds.has(findingId)) {
      setReviewedIds((current) => {
        const nextReviewed = new Set(current);
        nextReviewed.delete(findingId);
        return nextReviewed;
      });
      return;
    }

    setReviewedIds((current) => new Set([...current, findingId]));
    if (currentIndex === findingCount - 1) {
      setIsComplete(true);
      return;
    }
    setCurrentIndex((index) => index + 1);
  }, [currentIndex, findingCount, findingIds, reviewedIds]);

  const restart = useCallback(() => {
    setCurrentIndex(0);
    setIsComplete(false);
    setReviewedIds(new Set());
  }, []);

  return {
    currentIndex,
    isComplete,
    reviewedIds,
    goTo,
    next,
    previous,
    restart,
    toggleReviewed,
  };
}
