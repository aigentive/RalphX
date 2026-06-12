import os from "node:os";
import path from "node:path";

export function expandHome(value: string): string {
  if (!value.startsWith("~")) return value;
  return path.join(os.homedir(), value.slice(1));
}

export function normalizePathLike(value: string): string {
  return path.resolve(expandHome(value));
}

export function isWithin(root: string, candidate: string): boolean {
  const relative = path.relative(root, candidate);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

export function getPrimaryFilesystemRoot(): string {
  return normalizePathLike(process.cwd());
}

function parseConfiguredFilesystemReadRoots(value: string | undefined): string[] {
  if (!value || value.trim().length === 0) {
    return [];
  }

  const trimmed = value.trim();
  if (trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed)) {
        return parsed.filter((item): item is string => typeof item === "string");
      }
    } catch {
      return [];
    }
  }

  return trimmed
    .split(path.delimiter)
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

function isSafeFilesystemRoot(root: string): boolean {
  return path.isAbsolute(root) && root !== path.parse(root).root;
}

export function getConfiguredFilesystemReadRoots(): string[] {
  return parseConfiguredFilesystemReadRoots(process.env.RALPHX_FILESYSTEM_READ_ROOTS)
    .map(normalizePathLike)
    .filter(isSafeFilesystemRoot);
}

function uniqueRoots(roots: string[]): string[] {
  const seen = new Set<string>();
  const unique: string[] = [];

  for (const root of roots) {
    if (seen.has(root)) {
      continue;
    }
    seen.add(root);
    unique.push(root);
  }

  return unique;
}

export function getAllowedFilesystemRoots(): string[] {
  return uniqueRoots([
    getPrimaryFilesystemRoot(),
    ...getConfiguredFilesystemReadRoots(),
  ]);
}

export function resolveScopedFilesystemPath(inputPath: string, basePath?: string): string {
  const baseRoot = normalizePathLike(basePath ?? getPrimaryFilesystemRoot());
  const resolved = path.isAbsolute(inputPath) || inputPath.startsWith("~")
    ? normalizePathLike(inputPath)
    : normalizePathLike(path.join(baseRoot, inputPath));

  const allowedRoots = getAllowedFilesystemRoots();
  if (!allowedRoots.some((root) => isWithin(root, resolved))) {
    throw new Error(
      `Path "${inputPath}" resolves outside the allowed filesystem roots.`
    );
  }

  return resolved;
}
