export const MAX_GREP_CONTEXT_LINES = 20;
export function collectFileMatches(opts) {
    if (opts.outputMode === "files_with_matches") {
        const matched = opts.lines.some(opts.isMatch);
        const canEmit = matched && opts.remainingMatches > 0;
        return {
            output: canEmit ? [opts.relativePath] : [],
            matchCount: canEmit ? 1 : 0,
            capReached: matched && !canEmit,
        };
    }
    if (opts.outputMode === "count") {
        let count = 0;
        for (const line of opts.lines) {
            if (opts.isMatch(line)) {
                count += 1;
            }
        }
        const canEmit = count > 0 && opts.remainingMatches > 0;
        return {
            output: canEmit ? [`${opts.relativePath}:${count}`] : [],
            matchCount: canEmit ? 1 : 0,
            capReached: count > 0 && !canEmit,
        };
    }
    const matchIndices = [];
    let capReached = false;
    for (let index = 0; index < opts.lines.length; index += 1) {
        if (!opts.isMatch(opts.lines[index] ?? "")) {
            continue;
        }
        if (matchIndices.length >= opts.remainingMatches) {
            capReached = true;
            break;
        }
        matchIndices.push(index);
    }
    const selectedIndices = new Set();
    for (const matchIndex of matchIndices) {
        const first = Math.max(0, matchIndex - opts.contextLines);
        const last = Math.min(opts.lines.length - 1, matchIndex + opts.contextLines);
        for (let index = first; index <= last; index += 1) {
            selectedIndices.add(index);
        }
    }
    const matchIndexSet = new Set(matchIndices);
    const output = [];
    let previousIndex;
    for (const index of Array.from(selectedIndices).sort((a, b) => a - b)) {
        if (opts.contextLines > 0 &&
            previousIndex !== undefined &&
            index > previousIndex + 1) {
            output.push("--");
        }
        const separator = matchIndexSet.has(index) ? ":" : "-";
        output.push(`${opts.relativePath}${separator}${index + 1}${separator} ${opts.lines[index] ?? ""}`);
        previousIndex = index;
    }
    return {
        output,
        matchCount: matchIndices.length,
        capReached,
    };
}
//# sourceMappingURL=grep-matching.js.map