import path from "node:path";

export const MAX_NOTE_SAMPLES = 5;

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

type ExclusionKind =
  | "gitignored"
  | "hidden"
  | "symlink"
  | "depth"
  | "oversize";

type ExclusionCaps = {
  maxResults?: number;
  maxEntries?: number;
  maxFileBytes?: number;
  maxDepth?: number;
};

export function createExclusionCounters(): ExclusionCounters {
  return {
    gitignored: 0,
    hidden: 0,
    symlinks: 0,
    depthTruncatedDirs: 0,
    oversizeFiles: 0,
    resultCapReached: false,
    entryCapReached: false,
    samples: {
      gitignored: [],
      hidden: [],
      oversize: [],
    },
  };
}

export function recordExclusion(
  counters: ExclusionCounters,
  kind: ExclusionKind,
  relativePath?: string
): void {
  switch (kind) {
    case "gitignored":
      counters.gitignored += 1;
      recordSample(counters.samples.gitignored, relativePath);
      break;
    case "hidden":
      counters.hidden += 1;
      recordSample(counters.samples.hidden, relativePath);
      break;
    case "symlink":
      counters.symlinks += 1;
      break;
    case "depth":
      counters.depthTruncatedDirs += 1;
      break;
    case "oversize":
      counters.oversizeFiles += 1;
      recordSample(counters.samples.oversize, relativePath);
      break;
  }
}

export function formatExclusionNotes(
  counters: ExclusionCounters,
  caps: ExclusionCaps
): string[] {
  const notes: string[] = [];
  const pathExclusions: string[] = [];
  const includeFlags: string[] = [];

  if (counters.gitignored > 0) {
    pathExclusions.push(
      `${countLabel(counters.gitignored, "path")} excluded by .gitignore${formatSamples(counters.samples.gitignored)}`
    );
    includeFlags.push("respect_gitignore=false");
  }
  if (counters.hidden > 0) {
    pathExclusions.push(
      `${countLabel(counters.hidden, "hidden path")}${formatSamples(counters.samples.hidden)}`
    );
    includeFlags.push("include_hidden=true");
  }
  if (counters.symlinks > 0) {
    pathExclusions.push(`${countLabel(counters.symlinks, "symlink")} skipped`);
  }
  if (pathExclusions.length > 0) {
    const inclusionHint =
      includeFlags.length > 0
        ? ` Set ${includeFlags.join(" / ")} to include them.`
        : "";
    notes.push(`NOTE: ${pathExclusions.join(", ")}.${inclusionHint}`);
  }

  const capNotes: string[] = [];
  if (counters.resultCapReached && caps.maxResults !== undefined) {
    capNotes.push(
      `result cap reached (max_results=${caps.maxResults}); more matches exist`
    );
  }
  if (counters.entryCapReached && caps.maxEntries !== undefined) {
    capNotes.push(
      `entry cap reached (max_entries=${caps.maxEntries}); more entries exist`
    );
  }
  if (capNotes.length > 0) {
    notes.push(`NOTE: ${capNotes.join(". ")}.`);
  }

  const traversalLimits: string[] = [];
  if (counters.oversizeFiles > 0 && caps.maxFileBytes !== undefined) {
    traversalLimits.push(
      `${countLabel(counters.oversizeFiles, "file")} skipped for exceeding max_file_bytes=${caps.maxFileBytes}${formatSamples(counters.samples.oversize)}`
    );
  }
  if (counters.depthTruncatedDirs > 0 && caps.maxDepth !== undefined) {
    traversalLimits.push(
      `${countLabel(counters.depthTruncatedDirs, "directory", "directories")} not descended at max_depth=${caps.maxDepth}`
    );
  }
  if (traversalLimits.length > 0) {
    notes.push(`NOTE: ${traversalLimits.join(". ")}.`);
  }

  return notes;
}

function recordSample(samples: string[], relativePath?: string): void {
  if (!relativePath || samples.length >= MAX_NOTE_SAMPLES) {
    return;
  }
  const sample = path.basename(relativePath);
  if (sample.length > 0 && !samples.includes(sample)) {
    samples.push(sample);
  }
}

function countLabel(
  count: number,
  singular: string,
  plural = `${singular}s`
): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

function formatSamples(samples: string[]): string {
  return samples.length > 0 ? ` (${samples.join(", ")})` : "";
}
