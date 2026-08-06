export type GrepOutputMode = "content" | "files_with_matches" | "count";
export declare const MAX_GREP_CONTEXT_LINES = 20;
type CollectFileMatchesOptions = {
    relativePath: string;
    lines: string[];
    isMatch: (line: string) => boolean;
    contextLines: number;
    outputMode: GrepOutputMode;
    remainingMatches: number;
};
type CollectedFileMatches = {
    output: string[];
    matchCount: number;
    capReached: boolean;
};
export declare function collectFileMatches(opts: CollectFileMatchesOptions): CollectedFileMatches;
export {};
//# sourceMappingURL=grep-matching.d.ts.map