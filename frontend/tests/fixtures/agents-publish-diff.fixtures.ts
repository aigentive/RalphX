import type { Page } from "@playwright/test";

export async function installPagedPublishDiffRoute(page: Page) {
  await page.route("**/api/agent-workspaces/**/file-diff-page**", async (route) => {
    const url = new URL(route.request().url());
    const filePath = url.searchParams.get("path") ?? "frontend/src/Published.tsx";
    const rows = [
      {
        kind: "hunk_header",
        header: "@@ -1,2 +1,3 @@",
        old_start: 1,
        old_lines: 2,
        new_start: 1,
        new_lines: 3,
      },
      {
        kind: "line",
        line: {
          kind: "context",
          content: "export function publishedView() {",
          old_line_num: 1,
          new_line_num: 1,
        },
      },
      {
        kind: "line",
        line: {
          kind: "addition",
          content: "  const inlineRowsArePaged = true;",
          old_line_num: null,
          new_line_num: 2,
        },
      },
    ];
    const offset = Number(url.searchParams.get("offset") ?? "0");
    const limit = Number(url.searchParams.get("limit") ?? "200");
    const pageRows = rows.slice(offset, offset + limit);
    const nextOffset =
      offset + pageRows.length < rows.length ? offset + pageRows.length : null;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        file_path: filePath,
        language: "tsx",
        rows: pageRows,
        offset,
        limit,
        next_offset: nextOffset,
        total_rows: rows.length,
        old_total_lines: 2,
        new_total_lines: 3,
        is_binary: false,
      }),
    });
  });
}
