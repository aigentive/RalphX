import type { Dirent } from "node:fs";
import { type Ignore } from "ignore";
import { type ExclusionCounters } from "./exclusions.js";
export type TraversalOptions = {
    includeHidden: boolean;
    respectGitignore: boolean;
    maxWalkEntries: number;
    maxDepth: number;
};
type WalkContext = {
    root: string;
    options: TraversalOptions;
    counters: ExclusionCounters;
    visitedEntries: number;
};
export type FileEntry = {
    absolutePath: string;
    relativePath: string;
    dirent: Dirent;
};
type DirectoryScan = {
    ignoreMatcher: Ignore;
    effectiveIgnorePatterns: string[];
    entries: FileEntry[];
};
export declare function buildDirectoryScan(absoluteDir: string, relativeDir: string, inheritedIgnorePatterns: string[], options: TraversalOptions, counters: ExclusionCounters): Promise<DirectoryScan>;
export declare function walkFiles(root: string, options: TraversalOptions, counters: ExclusionCounters, onFile: (entry: FileEntry, context: WalkContext) => boolean | Promise<boolean>): Promise<void>;
export {};
//# sourceMappingURL=traversal.d.ts.map