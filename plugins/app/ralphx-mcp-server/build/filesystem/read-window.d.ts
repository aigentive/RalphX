export declare const DEFAULT_MAX_READ_LINES = 2000;
export declare const MAX_READ_LINES_CAP = 5000;
export declare const MAX_LINE_SCAN_BYTES: number;
export type LineWindowStop = "eof" | "end_line" | "max_lines" | "max_bytes" | "out_of_range";
export type LineWindow = {
    lines: string[];
    windowStart: number;
    windowEnd: number;
    totalLines: number;
    totalLinesExact: boolean;
    stoppedBy: LineWindowStop;
    requestedEndLine?: number;
};
type ReadLineWindowOptions = {
    startLine: number;
    endLine?: number;
    maxLines: number;
    maxBytes: number;
};
type FormatReadHeaderOptions = {
    displayPath: string;
    startLine: number;
    endLine?: number;
    maxLines: number;
    maxBytes: number;
};
export declare function readLineWindow(absolutePath: string, opts: ReadLineWindowOptions): Promise<LineWindow>;
export declare function formatReadHeader(window: LineWindow, opts: FormatReadHeaderOptions): string[];
export {};
//# sourceMappingURL=read-window.d.ts.map