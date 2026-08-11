import fs from "node:fs/promises";
export const DEFAULT_MAX_READ_LINES = 2000;
export const MAX_READ_LINES_CAP = 5000;
export const MAX_LINE_SCAN_BYTES = 8 * 1024 * 1024;
const READ_CHUNK_BYTES = 64 * 1024;
export async function readLineWindow(absolutePath, opts) {
    const windowEndTarget = Math.min(opts.endLine ?? Number.POSITIVE_INFINITY, opts.startLine + opts.maxLines - 1);
    const fileHandle = await fs.open(absolutePath, "r");
    const decoder = new TextDecoder("utf-8");
    const lines = [];
    let carry = "";
    let scannedBytes = 0;
    let filePosition = 0;
    let totalLines = 0;
    let totalLinesExact = true;
    let collectedBytes = 0;
    let stoppedAtMaxBytes = false;
    const processLine = (line) => {
        totalLines += 1;
        if (stoppedAtMaxBytes ||
            totalLines < opts.startLine ||
            totalLines > windowEndTarget) {
            return;
        }
        const lineBytes = Buffer.byteLength(line, "utf8") + 1;
        const isFirstWindowLine = lines.length === 0;
        if (!isFirstWindowLine && collectedBytes + lineBytes > opts.maxBytes) {
            stoppedAtMaxBytes = true;
            return;
        }
        lines.push(line);
        collectedBytes += lineBytes;
        if (collectedBytes > opts.maxBytes) {
            stoppedAtMaxBytes = true;
        }
    };
    try {
        while (scannedBytes < MAX_LINE_SCAN_BYTES) {
            const bytesToRead = Math.min(READ_CHUNK_BYTES, MAX_LINE_SCAN_BYTES - scannedBytes);
            const buffer = Buffer.allocUnsafe(bytesToRead);
            const { bytesRead } = await fileHandle.read(buffer, 0, bytesToRead, filePosition);
            if (bytesRead === 0) {
                break;
            }
            const chunk = buffer.subarray(0, bytesRead);
            if (chunk.includes(0)) {
                throw new Error(`File "${absolutePath}" appears to be binary.`);
            }
            filePosition += bytesRead;
            scannedBytes += bytesRead;
            const decoded = decoder.decode(chunk, { stream: true });
            const fragments = `${carry}${decoded}`.split("\n");
            carry = fragments.pop() ?? "";
            for (const line of fragments) {
                processLine(line);
            }
            if (bytesRead < bytesToRead) {
                break;
            }
            if (scannedBytes >= MAX_LINE_SCAN_BYTES) {
                totalLinesExact = false;
            }
        }
        if (totalLinesExact) {
            carry += decoder.decode();
            if (carry.length > 0) {
                processLine(carry);
            }
        }
    }
    finally {
        await fileHandle.close();
    }
    const windowStart = lines.length > 0 ? opts.startLine : 0;
    const windowEnd = lines.length > 0 ? opts.startLine + lines.length - 1 : 0;
    let stoppedBy;
    if (lines.length === 0) {
        stoppedBy = "out_of_range";
    }
    else if (stoppedAtMaxBytes) {
        stoppedBy = "max_bytes";
    }
    else {
        const finalRequestedLine = Math.min(opts.endLine ?? totalLines, totalLines);
        if (windowEnd === opts.startLine + opts.maxLines - 1 &&
            windowEnd < finalRequestedLine) {
            stoppedBy = "max_lines";
        }
        else if (opts.endLine !== undefined &&
            windowEnd === opts.endLine &&
            opts.endLine < totalLines) {
            stoppedBy = "end_line";
        }
        else {
            stoppedBy = "eof";
        }
    }
    return {
        lines,
        windowStart,
        windowEnd,
        totalLines,
        totalLinesExact,
        stoppedBy,
        ...(opts.endLine !== undefined && { requestedEndLine: opts.endLine }),
    };
}
export function formatReadHeader(window, opts) {
    const approximateSuffix = window.totalLinesExact ? "" : "+";
    const scanLimitNote = window.totalLinesExact
        ? ""
        : ` (line count capped at ${MAX_LINE_SCAN_BYTES} scan limit)`;
    let lineRange;
    if (window.totalLines === 0 && window.totalLinesExact) {
        lineRange = "LINES: none of 0 (file is empty)";
    }
    else if (window.stoppedBy === "out_of_range") {
        const explanation = window.totalLinesExact
            ? `start_line=${opts.startLine} is past end of file`
            : `start_line=${opts.startLine} was not reached before the scan limit`;
        lineRange = `LINES: none of ${window.totalLines}${approximateSuffix} (${explanation})${scanLimitNote}`;
    }
    else {
        lineRange = `LINES: ${window.windowStart}-${window.windowEnd} of ${window.totalLines}${approximateSuffix}${scanLimitNote}`;
    }
    const isTruncated = window.stoppedBy === "max_bytes" || window.stoppedBy === "max_lines";
    let truncation = "TRUNCATED: false";
    if (isTruncated) {
        const reason = window.stoppedBy === "max_bytes"
            ? `max_bytes=${opts.maxBytes}`
            : `max_lines=${opts.maxLines}`;
        const requestedEndClause = opts.endLine !== undefined && window.windowEnd < opts.endLine
            ? `; requested end_line=${opts.endLine}`
            : "";
        truncation = `TRUNCATED: true (stopped at ${reason}${requestedEndClause}; continue with start_line=${window.windowEnd + 1})`;
    }
    return [`FILE: ${opts.displayPath}`, lineRange, truncation];
}
//# sourceMappingURL=read-window.js.map