/**
 * useWorktreeParentVerdict - debounced, generation-guarded read-only check of
 * a worktree-parent directory candidate via `validate_worktree_parent`.
 * Never blocks typing: a superseded response is discarded via the generation
 * counter, and a request failure silently clears the verdict rather than
 * surfacing an error banner.
 */

import { useEffect, useRef, useState } from "react";
import { projectsApi } from "@/api/projects";
import type { WorktreeParentVerdict } from "@/types/worktree-parent";

const VERDICT_DEBOUNCE_MS = 300;

export function useWorktreeParentVerdict(
  path: string,
  repositoryRoot: string | undefined
): WorktreeParentVerdict | null {
  const [verdict, setVerdict] = useState<WorktreeParentVerdict | null>(null);
  const generationRef = useRef(0);

  useEffect(() => {
    const trimmed = path.trim();
    if (!trimmed) {
      setVerdict(null);
      return;
    }
    const generation = ++generationRef.current;
    const timer = setTimeout(() => {
      projectsApi
        .validateWorktreeParent({ path: trimmed, ...(repositoryRoot && { repositoryRoot }) })
        .then((result) => {
          if (generationRef.current !== generation) return;
          setVerdict(result);
        })
        .catch(() => {
          if (generationRef.current !== generation) return;
          setVerdict(null);
        });
    }, VERDICT_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [path, repositoryRoot]);

  return verdict;
}
