type GuideDiffRow =
  | {
      kind: "hunk_header";
      header: string;
      old_start: number;
      old_lines: number;
      new_start: number;
      new_lines: number;
    }
  | {
      kind: "line";
      line: {
        kind: "context" | "addition" | "deletion";
        content: string;
        old_line_num: number | null;
        new_line_num: number | null;
      };
    };

const GUIDE_WORKSPACE_DIFF_ROWS: readonly GuideDiffRow[] = [
  {
    kind: "hunk_header",
    header: "@@ -18,6 +18,9 @@",
    old_start: 18,
    old_lines: 6,
    new_start: 18,
    new_lines: 9,
  },
  {
    kind: "line",
    line: {
      kind: "context",
      content: "export function releaseChecklist() {",
      old_line_num: 18,
      new_line_num: 18,
    },
  },
  {
    kind: "line",
    line: {
      kind: "deletion",
      content: "  return [\"publish release notes\"];",
      old_line_num: 19,
      new_line_num: null,
    },
  },
  {
    kind: "line",
    line: {
      kind: "addition",
      content: "  return [\"run workspace review\", \"publish release notes\"];",
      old_line_num: null,
      new_line_num: 19,
    },
  },
  {
    kind: "line",
    line: {
      kind: "addition",
      content: "  // Keep the rollback owner visible before publishing.",
      old_line_num: null,
      new_line_num: 20,
    },
  },
  {
    kind: "line",
    line: {
      kind: "context",
      content: "}",
      old_line_num: 20,
      new_line_num: 21,
    },
  },
];

function languageFor(path: string): string {
  if (path.endsWith(".tsx")) return "tsx";
  if (path.endsWith(".rs")) return "rust";
  if (path.endsWith(".yaml") || path.endsWith(".yml")) return "yaml";
  return "text";
}

/** Raw HTTP response for paged workspace diffs in guide captures. */
export function mockGuideWorkspaceFileDiffPage(
  path: string,
  offset: number,
  limit: number,
) {
  const safeOffset = Math.max(0, offset);
  const safeLimit = Math.max(1, limit);
  const rows = GUIDE_WORKSPACE_DIFF_ROWS.slice(
    safeOffset,
    safeOffset + safeLimit,
  );
  const nextOffset = safeOffset + rows.length;
  return {
    file_path: path,
    language: languageFor(path),
    rows,
    offset: safeOffset,
    limit: safeLimit,
    next_offset:
      nextOffset < GUIDE_WORKSPACE_DIFF_ROWS.length ? nextOffset : null,
    total_rows: GUIDE_WORKSPACE_DIFF_ROWS.length,
    old_total_lines: 20,
    new_total_lines: 21,
    is_binary: false,
  };
}
