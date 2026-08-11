import os from "node:os";
import path from "node:path";
import fs from "node:fs/promises";
export function expandHome(value) {
    if (!value.startsWith("~"))
        return value;
    return path.join(os.homedir(), value.slice(1));
}
export function normalizePathLike(value) {
    return path.resolve(expandHome(value));
}
export function isWithin(root, candidate) {
    const relative = path.relative(root, candidate);
    return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}
export function getPrimaryFilesystemRoot() {
    return normalizePathLike(process.cwd());
}
function parseConfiguredFilesystemReadRoots(value) {
    if (!value || value.trim().length === 0) {
        return [];
    }
    const trimmed = value.trim();
    if (trimmed.startsWith("[")) {
        try {
            const parsed = JSON.parse(trimmed);
            if (Array.isArray(parsed)) {
                return parsed.filter((item) => typeof item === "string");
            }
        }
        catch {
            return [];
        }
    }
    return trimmed
        .split(path.delimiter)
        .map((item) => item.trim())
        .filter((item) => item.length > 0);
}
function isSafeFilesystemRoot(root) {
    return path.isAbsolute(root) && root !== path.parse(root).root;
}
export function getConfiguredFilesystemReadRoots() {
    return parseConfiguredFilesystemReadRoots(process.env.RALPHX_FILESYSTEM_READ_ROOTS)
        .map(normalizePathLike)
        .filter(isSafeFilesystemRoot);
}
function uniqueRoots(roots) {
    const seen = new Set();
    const unique = [];
    for (const root of roots) {
        if (seen.has(root)) {
            continue;
        }
        seen.add(root);
        unique.push(root);
    }
    return unique;
}
export function getAllowedFilesystemRoots() {
    return uniqueRoots([
        getPrimaryFilesystemRoot(),
        ...getConfiguredFilesystemReadRoots(),
    ]);
}
function filesystemPathDenied(inputPath) {
    return new Error(`Path "${inputPath}" resolves outside the allowed filesystem roots.`);
}
function errorCode(error) {
    return typeof error === "object" &&
        error !== null &&
        "code" in error &&
        typeof error.code === "string"
        ? error.code
        : undefined;
}
async function realConfiguredFilesystemReadRoots() {
    const roots = await Promise.all(getConfiguredFilesystemReadRoots().map(async (root) => {
        try {
            return await fs.realpath(root);
        }
        catch {
            return undefined;
        }
    }));
    return uniqueRoots(roots.filter((root) => root !== undefined));
}
async function realpathNearestExistingParent(candidate) {
    let current = path.dirname(candidate);
    while (true) {
        try {
            return await fs.realpath(current);
        }
        catch (error) {
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
export async function resolveEnforcedFilesystemPath(inputPath, displayPath) {
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
    }
    catch (error) {
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
export function resolveScopedFilesystemPath(inputPath, basePath) {
    const baseRoot = normalizePathLike(basePath ?? getPrimaryFilesystemRoot());
    const resolved = path.isAbsolute(inputPath) || inputPath.startsWith("~")
        ? normalizePathLike(inputPath)
        : normalizePathLike(path.join(baseRoot, inputPath));
    const allowedRoots = getAllowedFilesystemRoots();
    if (!allowedRoots.some((root) => isWithin(root, resolved))) {
        throw new Error(`Path "${inputPath}" resolves outside the allowed filesystem roots.`);
    }
    return resolved;
}
//# sourceMappingURL=path-policy.js.map