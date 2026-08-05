export declare const MAX_NOTE_SAMPLES = 5;
export type ExclusionCounters = {
    gitignored: number;
    hidden: number;
    symlinks: number;
    depthTruncatedDirs: number;
    oversizeFiles: number;
    resultCapReached: boolean;
    entryCapReached: boolean;
    samples: {
        gitignored: string[];
        hidden: string[];
        oversize: string[];
    };
};
type ExclusionKind = "gitignored" | "hidden" | "symlink" | "depth" | "oversize";
type ExclusionCaps = {
    maxResults?: number;
    maxEntries?: number;
    maxFileBytes?: number;
    maxDepth?: number;
};
export declare function createExclusionCounters(): ExclusionCounters;
export declare function recordExclusion(counters: ExclusionCounters, kind: ExclusionKind, relativePath?: string): void;
export declare function formatExclusionNotes(counters: ExclusionCounters, caps: ExclusionCaps): string[];
export {};
//# sourceMappingURL=exclusions.d.ts.map