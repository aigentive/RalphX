import type { Dirent } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import ignore, { type Ignore } from "ignore";
import {
  type ExclusionCounters,
  recordExclusion,
} from "./exclusions.js";

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

type QueueItem = {
  absoluteDir: string;
  relativeDir: string;
  inheritedIgnorePatterns: string[];
  depth: number;
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

function formatPathForIgnore(relativePath: string, isDirectory: boolean): string {
  if (relativePath === ".") {
    return isDirectory ? "./" : ".";
  }
  return isDirectory ? `${relativePath}/` : relativePath;
}

function hasHiddenSegment(relativePath: string): boolean {
  return relativePath
    .split("/")
    .filter((segment) => segment.length > 0 && segment !== "." && segment !== "..")
    .some((segment) => segment.startsWith("."));
}

function stripTrailingWhitespace(line: string): string {
  return line.replace(/\s+$/, "");
}

function convertIgnoreLineToRootPatterns(line: string, relativeDir: string): string[] {
  let raw = stripTrailingWhitespace(line);
  if (raw.length === 0) {
    return [];
  }
  if (raw.startsWith("\\#")) {
    raw = raw.slice(1);
  } else if (raw.startsWith("#")) {
    return [];
  }

  let negated = false;
  if (raw.startsWith("\\!")) {
    raw = raw.slice(1);
  } else if (raw.startsWith("!")) {
    negated = true;
    raw = raw.slice(1);
  }

  raw = raw.trim();
  if (raw.length === 0) {
    return [];
  }

  const directoryOnly = raw.endsWith("/");
  raw = raw.replace(/^\/+/, "").replace(/\/+$/, "").replace(/\\/g, "/");
  if (raw.length === 0) {
    return [];
  }

  const prefix = relativeDir === "." ? "" : `${relativeDir}/`;
  const rootedPattern = raw.includes("/") ? `${prefix}${raw}` : `${prefix}**/${raw}`;
  const patterns = directoryOnly ? [rootedPattern, `${rootedPattern}/**`] : [rootedPattern];

  return patterns.map((pattern) => (negated ? `!${pattern}` : pattern));
}

async function loadDirectoryIgnorePatterns(
  absoluteDir: string,
  relativeDir: string
): Promise<string[]> {
  const ignoreFiles = [".gitignore", ".ignore"];
  const patterns: string[] = [];

  for (const ignoreFile of ignoreFiles) {
    const absolutePath = path.resolve(absoluteDir, ignoreFile);
    try {
      const content = await fs.readFile(absolutePath, "utf8");
      for (const line of content.split(/\r?\n/)) {
        patterns.push(...convertIgnoreLineToRootPatterns(line, relativeDir));
      }
    } catch (error) {
      const code =
        typeof error === "object" &&
        error !== null &&
        "code" in error &&
        typeof (error as { code?: unknown }).code === "string"
          ? (error as { code: string }).code
          : undefined;
      if (code !== "ENOENT") {
        throw error;
      }
    }
  }

  return patterns;
}

export async function buildDirectoryScan(
  absoluteDir: string,
  relativeDir: string,
  inheritedIgnorePatterns: string[],
  options: TraversalOptions,
  counters: ExclusionCounters
): Promise<DirectoryScan> {
  const effectiveIgnorePatterns = options.respectGitignore
    ? [
        ...inheritedIgnorePatterns,
        ...(await loadDirectoryIgnorePatterns(absoluteDir, relativeDir)),
      ]
    : inheritedIgnorePatterns;
  const ignoreMatcher = ignore().add(effectiveIgnorePatterns);

  const dirEntries = await fs.readdir(absoluteDir, { withFileTypes: true });
  dirEntries.sort((a, b) => a.name.localeCompare(b.name));

  const entries: FileEntry[] = [];
  for (const dirent of dirEntries) {
    const absolutePath = path.resolve(absoluteDir, dirent.name);
    const relativePath =
      relativeDir === "."
        ? dirent.name
        : `${relativeDir}/${dirent.name}`;

    if (!options.includeHidden && hasHiddenSegment(relativePath)) {
      recordExclusion(counters, "hidden", relativePath);
      continue;
    }

    if (
      options.respectGitignore &&
      ignoreMatcher.ignores(formatPathForIgnore(relativePath, dirent.isDirectory()))
    ) {
      recordExclusion(counters, "gitignored", relativePath);
      continue;
    }

    entries.push({ absolutePath, relativePath, dirent });
  }

  return { ignoreMatcher, effectiveIgnorePatterns, entries };
}

function ensureWalkBudget(context: WalkContext): void {
  if (context.visitedEntries > context.options.maxWalkEntries) {
    throw new Error(
      `Traversal budget exceeded (${context.options.maxWalkEntries} entries). Narrow base_path or file_pattern.`
    );
  }
}

export async function walkFiles(
  root: string,
  options: TraversalOptions,
  counters: ExclusionCounters,
  onFile: (entry: FileEntry, context: WalkContext) => boolean | Promise<boolean>
): Promise<void> {
  const context: WalkContext = {
    root,
    options,
    counters,
    visitedEntries: 0,
  };
  const queue: QueueItem[] = [
    {
      absoluteDir: root,
      relativeDir: ".",
      inheritedIgnorePatterns: [],
      depth: 0,
    },
  ];
  let queueIndex = 0;

  while (queueIndex < queue.length) {
    const current = queue[queueIndex]!;
    queueIndex += 1;
    const scan = await buildDirectoryScan(
      current.absoluteDir,
      current.relativeDir,
      current.inheritedIgnorePatterns,
      options,
      counters
    );

    for (const entry of scan.entries) {
      context.visitedEntries += 1;
      ensureWalkBudget(context);

      if (entry.dirent.isSymbolicLink()) {
        recordExclusion(counters, "symlink");
        continue;
      }

      if (entry.dirent.isDirectory()) {
        if (current.depth < options.maxDepth) {
          queue.push({
            absoluteDir: entry.absolutePath,
            relativeDir: entry.relativePath,
            inheritedIgnorePatterns: scan.effectiveIgnorePatterns,
            depth: current.depth + 1,
          });
        } else {
          recordExclusion(counters, "depth");
        }
        continue;
      }

      if (!entry.dirent.isFile()) {
        continue;
      }

      const shouldContinue = await onFile(entry, context);
      if (!shouldContinue) {
        return;
      }
    }
  }
}
