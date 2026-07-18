import fs from "node:fs/promises";
import path from "node:path";
import ignore from "ignore";
import picomatch from "picomatch";
import { getPrimaryFilesystemRoot, normalizePathLike, resolveEnforcedFilesystemPath, } from "./path-policy.js";
const DEFAULT_MAX_READ_BYTES = 64 * 1024;
const MAX_READ_BYTES_CAP = 256 * 1024;
const DEFAULT_MAX_LIST_ENTRIES = 200;
const MAX_LIST_ENTRIES_CAP = 1_000;
const DEFAULT_MAX_GLOB_RESULTS = 200;
const MAX_GLOB_RESULTS_CAP = 2_000;
const DEFAULT_MAX_GREP_RESULTS = 100;
const MAX_GREP_RESULTS_CAP = 2_000;
const DEFAULT_MAX_FILE_BYTES_FOR_SEARCH = 256 * 1024;
const MAX_FILE_BYTES_FOR_SEARCH_CAP = 1024 * 1024;
const DEFAULT_MAX_WALK_ENTRIES = 20_000;
const MAX_WALK_ENTRIES_CAP = 100_000;
const DEFAULT_MAX_DEPTH = 8;
export const FILESYSTEM_TOOL_NAMES = [
    "fs_read_file",
    "fs_list_dir",
    "fs_grep",
    "fs_glob",
];
export const FILESYSTEM_TOOLS = [
    {
        name: "fs_read_file",
        description: "Read a local text file. Use this for direct source inspection without shell access. Absolute paths are accepted; relative paths resolve from the current MCP working directory.",
        inputSchema: {
            type: "object",
            properties: {
                path: {
                    type: "string",
                    description: "Absolute path or path relative to the current MCP working directory.",
                },
                start_line: {
                    type: "integer",
                    description: "Optional 1-based inclusive start line. Defaults to 1.",
                },
                end_line: {
                    type: "integer",
                    description: "Optional 1-based inclusive end line. Defaults to EOF.",
                },
                max_bytes: {
                    type: "integer",
                    description: `Optional byte cap for the read (default ${DEFAULT_MAX_READ_BYTES}, hard cap ${MAX_READ_BYTES_CAP}).`,
                },
            },
            required: ["path"],
            examples: [
                {
                    path: "src-tauri/src/http_server/handlers/coordination/mod.rs",
                    start_line: 1,
                    end_line: 80,
                },
            ],
        },
    },
    {
        name: "fs_list_dir",
        description: "List entries in a local directory. Defaults to ignoring gitignored and hidden entries so the result stays high-signal in large repos.",
        inputSchema: {
            type: "object",
            properties: {
                path: {
                    type: "string",
                    description: "Directory to inspect. Absolute path or path relative to the current MCP working directory. Defaults to '.'.",
                },
                include_hidden: {
                    type: "boolean",
                    description: "Include dotfiles and hidden directories. Defaults to false.",
                },
                respect_gitignore: {
                    type: "boolean",
                    description: "Respect .gitignore and .ignore files under the directory. Defaults to true.",
                },
                directories_only: {
                    type: "boolean",
                    description: "Return only directories. Defaults to false.",
                },
                max_entries: {
                    type: "integer",
                    description: `Maximum entries to return (default ${DEFAULT_MAX_LIST_ENTRIES}, hard cap ${MAX_LIST_ENTRIES_CAP}).`,
                },
            },
            examples: [
                {
                    path: "src-tauri/src",
                    directories_only: true,
                },
            ],
        },
    },
    {
        name: "fs_grep",
        description: "Search text content within local files. Uses ignore-aware traversal and bounded reads so it remains useful when shell access is disabled.",
        inputSchema: {
            type: "object",
            properties: {
                pattern: {
                    type: "string",
                    description: "Literal text to search for, or a regex pattern if regex=true.",
                },
                base_path: {
                    type: "string",
                    description: "Optional directory root for the search. Defaults to the current MCP working directory.",
                },
                file_pattern: {
                    type: "string",
                    description: "Optional glob-style filter such as '**/*.rs' or 'agents/**/*.md'. Defaults to '**/*'.",
                },
                case_sensitive: {
                    type: "boolean",
                    description: "Whether the search is case-sensitive. Defaults to false.",
                },
                regex: {
                    type: "boolean",
                    description: "Interpret pattern as a JavaScript regular expression. Defaults to false.",
                },
                include_hidden: {
                    type: "boolean",
                    description: "Include hidden files and directories in the traversal. Defaults to false.",
                },
                respect_gitignore: {
                    type: "boolean",
                    description: "Respect .gitignore and .ignore files. Defaults to true.",
                },
                max_results: {
                    type: "integer",
                    description: `Maximum number of matching lines to return (default ${DEFAULT_MAX_GREP_RESULTS}, hard cap ${MAX_GREP_RESULTS_CAP}).`,
                },
                max_file_bytes: {
                    type: "integer",
                    description: `Skip files larger than this byte size (default ${DEFAULT_MAX_FILE_BYTES_FOR_SEARCH}, hard cap ${MAX_FILE_BYTES_FOR_SEARCH_CAP}).`,
                },
                max_depth: {
                    type: "integer",
                    description: `Maximum directory traversal depth (default ${DEFAULT_MAX_DEPTH}).`,
                },
            },
            required: ["pattern"],
            examples: [
                {
                    pattern: "delegate_start",
                    base_path: "src-tauri/src",
                    file_pattern: "**/*.rs",
                    max_results: 20,
                },
            ],
        },
    },
    {
        name: "fs_glob",
        description: "List local files using production-grade glob matching. Defaults to ignoring gitignored and hidden paths so results stay close to ripgrep expectations.",
        inputSchema: {
            type: "object",
            properties: {
                pattern: {
                    type: "string",
                    description: "Glob-style pattern such as '**/*.rs' or 'agents/**/codex/*.md'.",
                },
                base_path: {
                    type: "string",
                    description: "Optional directory root for the glob. Defaults to the current MCP working directory.",
                },
                include_hidden: {
                    type: "boolean",
                    description: "Include hidden files and directories in the traversal. Defaults to false.",
                },
                respect_gitignore: {
                    type: "boolean",
                    description: "Respect .gitignore and .ignore files. Defaults to true.",
                },
                max_results: {
                    type: "integer",
                    description: `Maximum number of paths to return (default ${DEFAULT_MAX_GLOB_RESULTS}, hard cap ${MAX_GLOB_RESULTS_CAP}).`,
                },
                max_depth: {
                    type: "integer",
                    description: `Maximum directory traversal depth (default ${DEFAULT_MAX_DEPTH}).`,
                },
            },
            required: ["pattern"],
            examples: [
                {
                    pattern: "agents/**/codex/*.md",
                    max_results: 50,
                },
            ],
        },
    },
];
function isFilesystemToolName(name) {
    return FILESYSTEM_TOOL_NAMES.includes(name);
}
function getStringArg(args, key) {
    const value = args[key];
    return typeof value === "string" && value.length > 0 ? value : undefined;
}
function getBooleanArg(args, key, fallback) {
    const value = args[key];
    return typeof value === "boolean" ? value : fallback;
}
function getIntegerArg(args, key, fallback) {
    const value = args[key];
    if (typeof value !== "number" || !Number.isFinite(value)) {
        return fallback;
    }
    return Math.trunc(value);
}
function clampPositive(value, fallback, cap) {
    const normalized = Number.isFinite(value) && value > 0 ? Math.trunc(value) : fallback;
    if (cap !== undefined) {
        return Math.min(normalized, cap);
    }
    return normalized;
}
function clampNonNegative(value, fallback) {
    const normalized = Number.isFinite(value) && value >= 0 ? Math.trunc(value) : fallback;
    return normalized;
}
async function resolveReadOnlyExistingPath(inputPath, filesystemEnforced, basePath) {
    const baseRoot = normalizePathLike(basePath ?? getPrimaryFilesystemRoot());
    const displayPath = path.isAbsolute(inputPath) || inputPath.startsWith("~")
        ? normalizePathLike(inputPath)
        : path.resolve(baseRoot, inputPath);
    const safePath = filesystemEnforced
        ? await resolveEnforcedFilesystemPath(inputPath, displayPath)
        : await fs.realpath(displayPath);
    return {
        displayPath,
        safePath,
    };
}
function formatPathForIgnore(relativePath, isDirectory) {
    if (relativePath === ".") {
        return isDirectory ? "./" : ".";
    }
    return isDirectory ? `${relativePath}/` : relativePath;
}
function hasHiddenSegment(relativePath) {
    return relativePath
        .split("/")
        .filter((segment) => segment.length > 0 && segment !== "." && segment !== "..")
        .some((segment) => segment.startsWith("."));
}
function stripTrailingWhitespace(line) {
    return line.replace(/\s+$/, "");
}
function convertIgnoreLineToRootPatterns(line, relativeDir) {
    let raw = stripTrailingWhitespace(line);
    if (raw.length === 0) {
        return [];
    }
    if (raw.startsWith("\\#")) {
        raw = raw.slice(1);
    }
    else if (raw.startsWith("#")) {
        return [];
    }
    let negated = false;
    if (raw.startsWith("\\!")) {
        raw = raw.slice(1);
    }
    else if (raw.startsWith("!")) {
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
async function loadDirectoryIgnorePatterns(absoluteDir, relativeDir) {
    const ignoreFiles = [".gitignore", ".ignore"];
    const patterns = [];
    for (const ignoreFile of ignoreFiles) {
        const absolutePath = path.resolve(absoluteDir, ignoreFile);
        try {
            const content = await fs.readFile(absolutePath, "utf8");
            for (const line of content.split(/\r?\n/)) {
                patterns.push(...convertIgnoreLineToRootPatterns(line, relativeDir));
            }
        }
        catch (error) {
            const code = typeof error === "object" &&
                error !== null &&
                "code" in error &&
                typeof error.code === "string"
                ? error.code
                : undefined;
            if (code !== "ENOENT") {
                throw error;
            }
        }
    }
    return patterns;
}
async function buildDirectoryScan(absoluteDir, relativeDir, inheritedIgnorePatterns, options) {
    const effectiveIgnorePatterns = options.respectGitignore
        ? [
            ...inheritedIgnorePatterns,
            ...(await loadDirectoryIgnorePatterns(absoluteDir, relativeDir)),
        ]
        : inheritedIgnorePatterns;
    const ignoreMatcher = ignore().add(effectiveIgnorePatterns);
    const dirEntries = await fs.readdir(absoluteDir, { withFileTypes: true });
    dirEntries.sort((a, b) => a.name.localeCompare(b.name));
    const entries = [];
    for (const dirent of dirEntries) {
        const absolutePath = path.resolve(absoluteDir, dirent.name);
        const relativePath = relativeDir === "."
            ? dirent.name
            : `${relativeDir}/${dirent.name}`;
        if (!options.includeHidden && hasHiddenSegment(relativePath)) {
            continue;
        }
        if (options.respectGitignore &&
            ignoreMatcher.ignores(formatPathForIgnore(relativePath, dirent.isDirectory()))) {
            continue;
        }
        entries.push({ absolutePath, relativePath, dirent });
    }
    return { ignoreMatcher, effectiveIgnorePatterns, entries };
}
function ensureWalkBudget(context) {
    if (context.visitedEntries > context.options.maxWalkEntries) {
        throw new Error(`Traversal budget exceeded (${context.options.maxWalkEntries} entries). Narrow base_path or file_pattern.`);
    }
}
async function walkFiles(root, options, onFile) {
    const context = {
        root,
        options,
        visitedEntries: 0,
    };
    const queue = [
        {
            absoluteDir: root,
            relativeDir: ".",
            inheritedIgnorePatterns: [],
            depth: 0,
        },
    ];
    let queueIndex = 0;
    while (queueIndex < queue.length) {
        const current = queue[queueIndex];
        queueIndex += 1;
        const scan = await buildDirectoryScan(current.absoluteDir, current.relativeDir, current.inheritedIgnorePatterns, options);
        for (const entry of scan.entries) {
            context.visitedEntries += 1;
            ensureWalkBudget(context);
            if (entry.dirent.isSymbolicLink()) {
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
function readOnlyTraversalOptions(args) {
    return {
        includeHidden: getBooleanArg(args, "include_hidden", false),
        respectGitignore: getBooleanArg(args, "respect_gitignore", true),
        maxWalkEntries: clampPositive(getIntegerArg(args, "max_walk_entries", DEFAULT_MAX_WALK_ENTRIES), DEFAULT_MAX_WALK_ENTRIES, MAX_WALK_ENTRIES_CAP),
        maxDepth: clampNonNegative(getIntegerArg(args, "max_depth", DEFAULT_MAX_DEPTH), DEFAULT_MAX_DEPTH),
    };
}
async function readTextFile(absolutePath, maxBytes) {
    const fileHandle = await fs.open(absolutePath, "r");
    try {
        const buffer = Buffer.allocUnsafe(maxBytes + 1);
        const { bytesRead } = await fileHandle.read(buffer, 0, maxBytes + 1, 0);
        const sliced = buffer.subarray(0, Math.min(bytesRead, maxBytes));
        if (sliced.includes(0)) {
            throw new Error(`File "${absolutePath}" appears to be binary.`);
        }
        return {
            content: sliced.toString("utf8"),
            truncated: bytesRead > maxBytes,
        };
    }
    finally {
        await fileHandle.close();
    }
}
function formatByteSize(bytes) {
    if (bytes < 1024)
        return `${bytes} B`;
    if (bytes < 1024 * 1024)
        return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
async function handleReadFile(args, filesystemEnforced) {
    const requestedPath = getStringArg(args, "path");
    if (!requestedPath) {
        throw new Error("fs_read_file requires a non-empty path.");
    }
    const maxBytes = clampPositive(getIntegerArg(args, "max_bytes", DEFAULT_MAX_READ_BYTES), DEFAULT_MAX_READ_BYTES, MAX_READ_BYTES_CAP);
    const { displayPath, safePath } = await resolveReadOnlyExistingPath(requestedPath, filesystemEnforced);
    const stat = await fs.stat(safePath);
    if (!stat.isFile()) {
        throw new Error(`Path "${requestedPath}" is not a file.`);
    }
    const { content, truncated } = await readTextFile(safePath, maxBytes);
    const lines = content.split("\n");
    const totalLines = lines.length;
    const startLine = clampPositive(getIntegerArg(args, "start_line", 1), 1);
    const requestedEndLine = getIntegerArg(args, "end_line", totalLines);
    const endLine = Math.min(Math.max(requestedEndLine, startLine), totalLines);
    const slice = lines.slice(startLine - 1, endLine);
    const numbered = slice
        .map((line, index) => `${startLine + index}| ${line}`)
        .join("\n");
    const response = [
        `FILE: ${displayPath}`,
        `LINES: ${startLine}-${endLine}/${totalLines}`,
        truncated ? `TRUNCATED: true (max_bytes=${maxBytes})` : "TRUNCATED: false",
        "",
        numbered,
    ].join("\n");
    return {
        content: [{ type: "text", text: response }],
    };
}
async function handleListDir(args, filesystemEnforced) {
    const requestedPath = getStringArg(args, "path") ?? ".";
    const { displayPath, safePath } = await resolveReadOnlyExistingPath(requestedPath, filesystemEnforced);
    const stat = await fs.stat(safePath);
    if (!stat.isDirectory()) {
        throw new Error(`Path "${requestedPath}" is not a directory.`);
    }
    const options = readOnlyTraversalOptions(args);
    const directoriesOnly = getBooleanArg(args, "directories_only", false);
    const maxEntries = clampPositive(getIntegerArg(args, "max_entries", DEFAULT_MAX_LIST_ENTRIES), DEFAULT_MAX_LIST_ENTRIES, MAX_LIST_ENTRIES_CAP);
    const relativeRoot = ".";
    const scan = await buildDirectoryScan(safePath, relativeRoot, [], { ...options, maxWalkEntries: maxEntries });
    const lines = [];
    for (const entry of scan.entries) {
        if (entry.dirent.isSymbolicLink()) {
            continue;
        }
        if (directoriesOnly && !entry.dirent.isDirectory()) {
            continue;
        }
        if (entry.dirent.isDirectory()) {
            lines.push(`DIR  ${path.basename(entry.relativePath)}/`);
        }
        else if (entry.dirent.isFile()) {
            const entryStat = await fs.stat(entry.absolutePath);
            lines.push(`FILE ${path.basename(entry.relativePath)} (${formatByteSize(entryStat.size)})`);
        }
        if (lines.length >= maxEntries) {
            break;
        }
    }
    const response = [
        `DIRECTORY: ${displayPath}`,
        `ENTRIES: ${lines.length}`,
        `DIRECTORIES_ONLY: ${directoriesOnly}`,
        `INCLUDE_HIDDEN: ${options.includeHidden}`,
        `RESPECT_GITIGNORE: ${options.respectGitignore}`,
        "",
        ...lines,
    ].join("\n");
    return {
        content: [{ type: "text", text: response }],
    };
}
async function handleGlob(args, filesystemEnforced) {
    const pattern = getStringArg(args, "pattern");
    if (!pattern) {
        throw new Error("fs_glob requires a non-empty pattern.");
    }
    const basePath = getStringArg(args, "base_path") ?? ".";
    const { displayPath: displayRoot, safePath: safeRoot } = await resolveReadOnlyExistingPath(basePath, filesystemEnforced);
    const rootStat = await fs.stat(safeRoot);
    if (!rootStat.isDirectory()) {
        throw new Error(`Base path "${basePath}" is not a directory.`);
    }
    const options = readOnlyTraversalOptions(args);
    const maxResults = clampPositive(getIntegerArg(args, "max_results", DEFAULT_MAX_GLOB_RESULTS), DEFAULT_MAX_GLOB_RESULTS, MAX_GLOB_RESULTS_CAP);
    const matcher = picomatch(pattern, {
        dot: options.includeHidden,
    });
    const matches = [];
    await walkFiles(safeRoot, options, async ({ relativePath }) => {
        if (matcher(relativePath)) {
            matches.push(relativePath);
            if (matches.length >= maxResults) {
                return false;
            }
        }
        return true;
    });
    const response = [
        `ROOT: ${displayRoot}`,
        `PATTERN: ${pattern}`,
        `MATCHES: ${matches.length}`,
        `INCLUDE_HIDDEN: ${options.includeHidden}`,
        `RESPECT_GITIGNORE: ${options.respectGitignore}`,
        "",
        ...matches,
    ].join("\n");
    return {
        content: [{ type: "text", text: response }],
    };
}
async function handleGrep(args, filesystemEnforced) {
    const pattern = getStringArg(args, "pattern");
    if (!pattern) {
        throw new Error("fs_grep requires a non-empty pattern.");
    }
    const basePath = getStringArg(args, "base_path") ?? ".";
    const filePattern = getStringArg(args, "file_pattern") ?? "**/*";
    const caseSensitive = getBooleanArg(args, "case_sensitive", false);
    const regexMode = getBooleanArg(args, "regex", false);
    const options = readOnlyTraversalOptions(args);
    const maxResults = clampPositive(getIntegerArg(args, "max_results", DEFAULT_MAX_GREP_RESULTS), DEFAULT_MAX_GREP_RESULTS, MAX_GREP_RESULTS_CAP);
    const maxFileBytes = clampPositive(getIntegerArg(args, "max_file_bytes", DEFAULT_MAX_FILE_BYTES_FOR_SEARCH), DEFAULT_MAX_FILE_BYTES_FOR_SEARCH, MAX_FILE_BYTES_FOR_SEARCH_CAP);
    const { displayPath: displayRoot, safePath: safeRoot } = await resolveReadOnlyExistingPath(basePath, filesystemEnforced);
    const rootStat = await fs.stat(safeRoot);
    if (!rootStat.isDirectory()) {
        throw new Error(`Base path "${basePath}" is not a directory.`);
    }
    const fileMatcher = picomatch(filePattern, {
        dot: options.includeHidden,
    });
    const regex = regexMode
        ? new RegExp(pattern, caseSensitive ? "g" : "gi")
        : null;
    const literalNeedle = caseSensitive ? pattern : pattern.toLowerCase();
    const matches = [];
    await walkFiles(safeRoot, options, async ({ absolutePath, relativePath }) => {
        if (!fileMatcher(relativePath)) {
            return true;
        }
        const stat = await fs.stat(absolutePath);
        if (stat.size > maxFileBytes) {
            return true;
        }
        const { content } = await readTextFile(absolutePath, maxFileBytes);
        const lines = content.split("\n");
        for (let index = 0; index < lines.length; index += 1) {
            const line = lines[index] ?? "";
            const matched = regex
                ? regex.test(line)
                : (caseSensitive ? line : line.toLowerCase()).includes(literalNeedle);
            if (!matched) {
                if (regex) {
                    regex.lastIndex = 0;
                }
                continue;
            }
            matches.push(`${relativePath}:${index + 1}: ${line}`);
            if (regex) {
                regex.lastIndex = 0;
            }
            if (matches.length >= maxResults) {
                return false;
            }
        }
        return true;
    });
    const response = [
        `ROOT: ${displayRoot}`,
        `PATTERN: ${pattern}`,
        `FILE_PATTERN: ${filePattern}`,
        `MATCHES: ${matches.length}`,
        `INCLUDE_HIDDEN: ${options.includeHidden}`,
        `RESPECT_GITIGNORE: ${options.respectGitignore}`,
        "",
        ...matches,
    ].join("\n");
    return {
        content: [{ type: "text", text: response }],
    };
}
export async function handleFilesystemToolCall(name, rawArgs, runtimeContext) {
    if (!isFilesystemToolName(name)) {
        throw new Error(`Unknown filesystem tool "${name}".`);
    }
    const args = rawArgs && typeof rawArgs === "object" && !Array.isArray(rawArgs)
        ? rawArgs
        : {};
    switch (name) {
        case "fs_read_file":
            return handleReadFile(args, runtimeContext.filesystemEnforced);
        case "fs_list_dir":
            return handleListDir(args, runtimeContext.filesystemEnforced);
        case "fs_grep":
            return handleGrep(args, runtimeContext.filesystemEnforced);
        case "fs_glob":
            return handleGlob(args, runtimeContext.filesystemEnforced);
    }
}
export function formatFilesystemToolError(error) {
    const message = error instanceof Error ? error.message : String(error);
    const primaryRoot = normalizePathLike(getPrimaryFilesystemRoot());
    return {
        content: [
            {
                type: "text",
                text: [
                    `ERROR: ${message}`,
                    "Path resolution: absolute paths are read directly; relative paths resolve from:",
                    `- ${primaryRoot}`,
                ].join("\n"),
            },
        ],
        isError: true,
    };
}
//# sourceMappingURL=filesystem-tools.js.map