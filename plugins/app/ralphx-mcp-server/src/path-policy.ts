import os from "node:os";
import path from "node:path";
import fs from "node:fs/promises";

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

function filesystemPathDenied(inputPath: string): Error {
  return new Error(
    `Path "${inputPath}" resolves outside the allowed filesystem roots.`
  );
}

function errorCode(error: unknown): string | undefined {
  return typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof (error as { code?: unknown }).code === "string"
    ? (error as { code: string }).code
    : undefined;
}

async function realConfiguredFilesystemReadRoots(): Promise<string[]> {
  const roots = await Promise.all(
    getConfiguredFilesystemReadRoots().map(async (root) => {
      try {
        return await fs.realpath(root);
      } catch {
        return undefined;
      }
    })
  );
  return uniqueRoots(roots.filter((root): root is string => root !== undefined));
}

async function realpathNearestExistingParent(candidate: string): Promise<string | undefined> {
  let current = path.dirname(candidate);

  while (true) {
    try {
      return await fs.realpath(current);
    } catch (error) {
      if (errorCode(error) !== "ENOENT") {
        throw error;
      }
    }

    const parent = path.dirname(current);
    if (parent === current) {
      return undefined;
    }
    current = parent;
  }
}

export async function resolveEnforcedFilesystemPath(
  inputPath: string,
  displayPath: string
): Promise<string> {
  const allowedRoots = await realConfiguredFilesystemReadRoots();
  if (allowedRoots.length === 0) {
    throw filesystemPathDenied(inputPath);
  }

  try {
    const realTarget = await fs.realpath(displayPath);
    if (!allowedRoots.some((root) => isWithin(root, realTarget))) {
      throw filesystemPathDenied(inputPath);
    }
    return realTarget;
  } catch (error) {
    if (errorCode(error) !== "ENOENT") {
      throw error;
    }

    const realParent = await realpathNearestExistingParent(displayPath);
    if (!realParent || !allowedRoots.some((root) => isWithin(root, realParent))) {
      throw filesystemPathDenied(inputPath);
    }
    throw error;
  }
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
