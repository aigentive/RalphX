import os from "node:os";
import path from "node:path";
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