import fs from "node:fs/promises";
import path from "node:path";
import picomatch from "picomatch";
import { createExclusionCounters, formatExclusionNotes, recordExclusion, } from "./filesystem/exclusions.js";
import { collectFileMatches, MAX_GREP_CONTEXT_LINES, } from "./filesystem/grep-matching.js";
import { DEFAULT_MAX_READ_LINES, formatReadHeader, MAX_READ_LINES_CAP, readLineWindow, } from "./filesystem/read-window.js";
import { buildDirectoryScan, walkFiles, } from "./filesystem/traversal.js";
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
        description: "Read a local text file, optionally a bounded line window at any offset. Absolute paths are accepted; relative paths resolve from the current MCP working directory.",
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
                max_lines: {
                    type: "integer",
                    description: `Maximum number of lines to return (default ${DEFAULT_MAX_READ_LINES}, hard cap ${MAX_READ_LINES_CAP}). Use with start_line for a bounded window anywhere in a large file.`,
                },
                max_bytes: {
                    type: "integer",
                    description: `Byte cap on the returned window, not on how far into the file the read may start (default ${DEFAULT_MAX_READ_BYTES}, hard cap ${MAX_READ_BYTES_CAP}).`,
                },
            },
            required: ["path"],
            examples: [
                {
                    path: "src-tauri/src/http_server/handlers/coordination/mod.rs",
                    start_line: 1,
                    end_line: 80,
                },
                {
                    path: "frontend/src/components/AgentsSidebar.tsx",
                    start_line: 2699,
                    max_lines: 60,
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
                    description: `Maximum matching lines in content mode, or matching files in other output modes (default ${DEFAULT_MAX_GREP_RESULTS}, hard cap ${MAX_GREP_RESULTS_CAP}). Context lines do not count toward this cap.`,
                },
                max_file_bytes: {
                    type: "integer",
                    description: `Skip files larger than this byte size (default ${DEFAULT_MAX_FILE_BYTES_FOR_SEARCH}, hard cap ${MAX_FILE_BYTES_FOR_SEARCH_CAP}).`,
                },
                max_depth: {
                    type: "integer",
                    description: `Maximum directory traversal depth (default ${DEFAULT_MAX_DEPTH}).`,
                },
                context_lines: {
                    type: "integer",
                    description: `Lines of context before and after each match (default 0, hard cap ${MAX_GREP_CONTEXT_LINES}). Context lines use "path-N- text"; matches use "path:N: text".`,
                },
                output_mode: {
                    type: "string",
                    enum: ["content", "files_with_matches", "count"],
                    description: "content (default) returns matching lines; files_with_matches returns one path per matching file; count returns path:count per file.",
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
    const startLine = clampPositive(getIntegerArg(args, "start_line", 1), 1);
    const rawEndLine = getIntegerArg(args, "end_line", 0);
    const endLine = rawEndLine > 0 ? Math.max(rawEndLine, startLine) : undefined;
    const maxLines = clampPositive(getIntegerArg(args, "max_lines", DEFAULT_MAX_READ_LINES), DEFAULT_MAX_READ_LINES, MAX_READ_LINES_CAP);
    const window = await readLineWindow(safePath, {
        startLine,
        ...(endLine !== undefined && { endLine }),
        maxLines,
        maxBytes,
    });
    const numbered = window.lines
        .map((line, index) => `${window.windowStart + index}| ${line}`)
        .join("\n");
    const response = [
        ...formatReadHeader(window, {
            displayPath,
            startLine,
            ...(endLine !== undefined && { endLine }),
            maxLines,
            maxBytes,
        }),
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
    const counters = createExclusionCounters();
    const scan = await buildDirectoryScan(safePath, relativeRoot, [], { ...options, maxWalkEntries: maxEntries }, counters);
    const lines = [];
    for (const entry of scan.entries) {
        if (entry.dirent.isSymbolicLink()) {
            recordExclusion(counters, "symlink");
            continue;
        }
        if (directoriesOnly && !entry.dirent.isDirectory()) {
            continue;
        }
        if (!entry.dirent.isDirectory() && !entry.dirent.isFile()) {
            continue;
        }
        if (lines.length >= maxEntries) {
            counters.entryCapReached = true;
            break;
        }
        if (entry.dirent.isDirectory()) {
            lines.push(`DIR  ${path.basename(entry.relativePath)}/`);
        }
        else if (entry.dirent.isFile()) {
            const entryStat = await fs.stat(entry.absolutePath);
            lines.push(`FILE ${path.basename(entry.relativePath)} (${formatByteSize(entryStat.size)})`);
        }
    }
    const response = [
        `DIRECTORY: ${displayPath}`,
        `ENTRIES: ${lines.length}`,
        `DIRECTORIES_ONLY: ${directoriesOnly}`,
        `INCLUDE_HIDDEN: ${options.includeHidden}`,
        `RESPECT_GITIGNORE: ${options.respectGitignore}`,
        ...formatExclusionNotes(counters, { maxEntries }),
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
    const counters = createExclusionCounters();
    await walkFiles(safeRoot, options, counters, async ({ relativePath }) => {
        if (matcher(relativePath)) {
            if (matches.length >= maxResults) {
                counters.resultCapReached = true;
                return false;
            }
            matches.push(relativePath);
        }
        return true;
    });
    const response = [
        `ROOT: ${displayRoot}`,
        `PATTERN: ${pattern}`,
        `MATCHES: ${matches.length}`,
        `INCLUDE_HIDDEN: ${options.includeHidden}`,
        `RESPECT_GITIGNORE: ${options.respectGitignore}`,
        ...formatExclusionNotes(counters, {
            maxResults,
            maxDepth: options.maxDepth,
        }),
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
    const contextLines = Math.min(clampNonNegative(getIntegerArg(args, "context_lines", 0), 0), MAX_GREP_CONTEXT_LINES);
    const rawOutputMode = getStringArg(args, "output_mode") ?? "content";
    const outputMode = [
        "content",
        "files_with_matches",
        "count",
    ].includes(rawOutputMode)
        ? rawOutputMode
        : "content";
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
    const counters = createExclusionCounters();
    let emittedMatches = 0;
    const isMatch = (line) => {
        if (regex) {
            regex.lastIndex = 0;
            const matched = regex.test(line);
            regex.lastIndex = 0;
            return matched;
        }
        return (caseSensitive ? line : line.toLowerCase()).includes(literalNeedle);
    };
    await walkFiles(safeRoot, options, counters, async ({ absolutePath, relativePath }) => {
        if (!fileMatcher(relativePath)) {
            return true;
        }
        const stat = await fs.stat(absolutePath);
        if (stat.size > maxFileBytes) {
            recordExclusion(counters, "oversize", relativePath);
            return true;
        }
        const { content } = await readTextFile(absolutePath, maxFileBytes);
        const collected = collectFileMatches({
            relativePath,
            lines: content.split("\n"),
            isMatch,
            contextLines,
            outputMode,
            remainingMatches: maxResults - emittedMatches,
        });
        matches.push(...collected.output);
        emittedMatches += collected.matchCount;
        if (collected.capReached) {
            counters.resultCapReached = true;
            return false;
        }
        return true;
    });
    const response = [
        `ROOT: ${displayRoot}`,
        `PATTERN: ${pattern}`,
        `FILE_PATTERN: ${filePattern}`,
        `OUTPUT_MODE: ${outputMode}`,
        `MATCHES: ${emittedMatches}`,
        `INCLUDE_HIDDEN: ${options.includeHidden}`,
        `RESPECT_GITIGNORE: ${options.respectGitignore}`,
        ...formatExclusionNotes(counters, {
            maxResults,
            maxFileBytes,
            maxDepth: options.maxDepth,
        }),
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