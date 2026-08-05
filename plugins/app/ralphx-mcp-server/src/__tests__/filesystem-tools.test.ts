import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  formatFilesystemToolError,
  handleFilesystemToolCall as handleFilesystemToolCallWithContext,
} from "../filesystem-tools.js";

const handleFilesystemToolCall = (
  name: string,
  rawArgs: unknown,
  runtimeContext: { filesystemEnforced: boolean }
) => handleFilesystemToolCallWithContext(name, rawArgs, runtimeContext);
const filesystemNotEnforced = { filesystemEnforced: false } as const;

describe("filesystem tools", () => {
  const tempDirs: string[] = [];
  const originalCwd = process.cwd();
  const originalFilesystemReadRoots = process.env.RALPHX_FILESYSTEM_READ_ROOTS;

  afterEach(() => {
    process.chdir(originalCwd);
    if (originalFilesystemReadRoots === undefined) {
      delete process.env.RALPHX_FILESYSTEM_READ_ROOTS;
    } else {
      process.env.RALPHX_FILESYSTEM_READ_ROOTS = originalFilesystemReadRoots;
    }

    for (const dir of tempDirs.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  function makeWorkspace(): string {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-tools-"));
    const canonicalDir = fs.realpathSync(dir);
    tempDirs.push(dir);
    process.chdir(canonicalDir);
    return canonicalDir;
  }

  function writeNumberedFile(target: string, lineCount: number, lineWidth = 24): void {
    fs.mkdirSync(path.dirname(target), { recursive: true });
    const lines = Array.from({ length: lineCount }, (_, index) => {
      const prefix = `line ${index + 1} `;
      return `${prefix}${"x".repeat(Math.max(0, lineWidth - prefix.length))}`;
    });
    fs.writeFileSync(target, lines.join("\n"));
  }

  it("reads a relative file from the current working directory", async () => {
    const root = makeWorkspace();
    const target = path.join(root, "src", "sample.ts");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "line one\nline two\nline three\n");

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      {
        path: "src/sample.ts",
        start_line: 2,
        end_line: 3,
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(`FILE: ${target}`);
    expect(text).toContain("LINES: 2-3 of 3");
    expect(text).toContain("2| line two");
    expect(text).toContain("3| line three");
  });

  it("reads and lists absolute paths outside the workspace without extra read roots", async () => {
    makeWorkspace();
    const projectRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-project-"))
    );
    tempDirs.push(projectRoot);
    delete process.env.RALPHX_FILESYSTEM_READ_ROOTS;

    const target = path.join(projectRoot, ".artifacts", "specs", "ralphx-cli", "tracker.md");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "# CLI tracker\n\nreadable from project checkout\n");

    const readResult = await handleFilesystemToolCall(
      "fs_read_file",
      { path: target },
      filesystemNotEnforced
    );
    const readText = readResult.content[0]?.text ?? "";
    expect(readText).toContain(`FILE: ${target}`);
    expect(readText).toContain("readable from project checkout");

    const listResult = await handleFilesystemToolCall(
      "fs_list_dir",
      {
        path: path.dirname(target),
        include_hidden: true,
      },
      filesystemNotEnforced
    );
    const listText = listResult.content[0]?.text ?? "";
    expect(listText).toContain("FILE tracker.md");
  });

  it("lists a directory while respecting hidden files and gitignore by default", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, ".gitignore"), "dist/\nsecret.log\n");
    fs.mkdirSync(path.join(root, "src"), { recursive: true });
    fs.mkdirSync(path.join(root, "dist"), { recursive: true });
    fs.writeFileSync(path.join(root, "visible.ts"), "export const ok = true;\n");
    fs.writeFileSync(path.join(root, "secret.log"), "hidden by ignore\n");
    fs.writeFileSync(path.join(root, ".env"), "TOKEN=1\n");

    const result = await handleFilesystemToolCall(
      "fs_list_dir",
      { path: "." },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    const body = text.split("\n\n")[1] ?? "";
    expect(text).toContain("DIR  src/");
    expect(text).toContain("FILE visible.ts");
    expect(body).not.toContain("dist/");
    expect(body).not.toContain("secret.log");
    expect(body).not.toContain(".env");
  });

  it("reads relative paths that resolve outside the workspace", async () => {
    const root = makeWorkspace();
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-outside-"));
    tempDirs.push(outsideDir);
    const outsideFile = path.join(outsideDir, "secret.txt");
    fs.writeFileSync(outsideFile, "external context\n");

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      { path: path.relative(root, outsideFile) },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(`FILE: ${outsideFile}`);
    expect(text).toContain("external context");
  });

  it("reads symlinked file paths that point outside the workspace", async () => {
    const root = makeWorkspace();
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-link-outside-"));
    tempDirs.push(outsideDir);
    const outsideFile = path.join(outsideDir, "secret.txt");
    fs.writeFileSync(outsideFile, "symlinked context\n");

    const symlinkPath = path.join(root, "src", "escape.txt");
    fs.mkdirSync(path.dirname(symlinkPath), { recursive: true });
    fs.symlinkSync(outsideFile, symlinkPath);

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      { path: "src/escape.txt" },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(`FILE: ${symlinkPath}`);
    expect(text).toContain("symlinked context");
  });

  it("globs symlinked base paths that point outside the workspace", async () => {
    const root = makeWorkspace();
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-base-outside-"));
    tempDirs.push(outsideDir);
    fs.writeFileSync(path.join(outsideDir, "secret.ts"), "export const secret = true;\n");

    const symlinkPath = path.join(root, "linked");
    fs.symlinkSync(outsideDir, symlinkPath);

    const result = await handleFilesystemToolCall(
      "fs_glob",
      {
        base_path: "linked",
        pattern: "**/*.ts",
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(`ROOT: ${path.join(root, "linked")}`);
    expect(text).toContain("secret.ts");
  });

  it("greps and globs absolute base paths outside the workspace", async () => {
    makeWorkspace();
    const projectRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-search-project-"))
    );
    tempDirs.push(projectRoot);
    const sourceFile = path.join(projectRoot, "service", "index.ts");
    fs.mkdirSync(path.dirname(sourceFile), { recursive: true });
    fs.writeFileSync(sourceFile, "export const externalNeedle = true;\n");

    const grepResult = await handleFilesystemToolCall(
      "fs_grep",
      {
        pattern: "externalNeedle",
        base_path: projectRoot,
        file_pattern: "**/*.ts",
      },
      filesystemNotEnforced
    );
    const grepText = grepResult.content[0]?.text ?? "";
    expect(grepText).toContain(`ROOT: ${projectRoot}`);
    expect(grepText).toContain("service/index.ts:1: export const externalNeedle = true;");

    const globResult = await handleFilesystemToolCall(
      "fs_glob",
      {
        base_path: projectRoot,
        pattern: "**/*.ts",
      },
      filesystemNotEnforced
    );
    const globText = globResult.content[0]?.text ?? "";
    expect(globText).toContain(`ROOT: ${projectRoot}`);
    expect(globText).toContain("service/index.ts");
  });

  it("greps within the current working directory using a file pattern", async () => {
    const root = makeWorkspace();
    const rustFile = path.join(root, "src-tauri", "src", "main.rs");
    fs.mkdirSync(path.dirname(rustFile), { recursive: true });
    fs.writeFileSync(
      rustFile,
      "fn main() {\n    println!(\"delegate_start\");\n}\n"
    );
    fs.writeFileSync(path.join(root, "README.md"), "delegate_start\n");
    fs.writeFileSync(path.join(root, ".gitignore"), "ignored.rs\n");
    fs.writeFileSync(path.join(root, "ignored.rs"), "delegate_start\n");

    const result = await handleFilesystemToolCall(
      "fs_grep",
      {
        pattern: "delegate_start",
        base_path: ".",
        file_pattern: "**/*.rs",
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    const body = text.split("\n\n")[1] ?? "";
    expect(text).toContain("FILE_PATTERN: **/*.rs");
    expect(text).toContain("src-tauri/src/main.rs:2:     println!(\"delegate_start\");");
    expect(body).not.toContain("README.md");
    expect(body).not.toContain("ignored.rs");
  });

  it("globs within the current working directory", async () => {
    const root = makeWorkspace();
    const first = path.join(root, "agents", "one", "codex", "prompt.md");
    const second = path.join(root, "agents", "two", "codex", "prompt.md");
    fs.mkdirSync(path.dirname(first), { recursive: true });
    fs.mkdirSync(path.dirname(second), { recursive: true });
    fs.writeFileSync(first, "# one\n");
    fs.writeFileSync(second, "# two\n");
    fs.writeFileSync(path.join(root, ".gitignore"), "agents/two/\n");

    const result = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "agents/**/codex/*.md" },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("agents/one/codex/prompt.md");
    expect(text).not.toContain("agents/two/codex/prompt.md");
  });

  it("respects max_depth during recursive glob traversal", async () => {
    const root = makeWorkspace();
    const shallow = path.join(root, "src", "one.ts");
    const deep = path.join(root, "src", "nested", "two.ts");
    fs.mkdirSync(path.dirname(deep), { recursive: true });
    fs.writeFileSync(shallow, "export const one = 1;\n");
    fs.writeFileSync(deep, "export const two = 2;\n");

    const result = await handleFilesystemToolCall(
      "fs_glob",
      {
        base_path: "src",
        pattern: "**/*.ts",
        max_depth: 0,
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("one.ts");
    expect(text).not.toContain("nested/two.ts");
  });

  it("caps file reads without loading the entire file into the response", async () => {
    const root = makeWorkspace();
    const target = path.join(root, "src", "large.ts");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, `${"x".repeat(4096)}\n${"y".repeat(4096)}\n`);

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      {
        path: "src/large.ts",
        max_bytes: 128,
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(
      "TRUNCATED: true (stopped at max_bytes=128; continue with start_line=2)"
    );
    expect(text).toContain("LINES: 1-1 of 2");
  });

  it("reads a high-offset window from a file larger than max_bytes", async () => {
    const root = makeWorkspace();
    writeNumberedFile(path.join(root, "src", "large.ts"), 3200, 40);

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      {
        path: "src/large.ts",
        start_line: 2845,
        max_lines: 60,
        max_bytes: 65_536,
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toMatch(/LINES: 2845-\d+ of 3200/);
    expect(text).toContain("2845| line 2845 ");
  });

  it("reports an unreached end_line and a resume point when max_bytes truncates", async () => {
    const root = makeWorkspace();
    writeNumberedFile(path.join(root, "large.ts"), 100, 40);

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      {
        path: "large.ts",
        start_line: 10,
        end_line: 80,
        max_bytes: 100,
      },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toMatch(
      /TRUNCATED: true \(stopped at max_bytes=100; requested end_line=80; continue with start_line=\d+\)/
    );
  });

  it("explains when start_line is past EOF", async () => {
    const root = makeWorkspace();
    writeNumberedFile(path.join(root, "large.ts"), 3200);

    const result = await handleFilesystemToolCall(
      "fs_read_file",
      { path: "large.ts", start_line: 5000 },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(
      "LINES: none of 3200 (start_line=5000 is past end of file)"
    );
    expect(text).toContain("TRUNCATED: false");
  });

  it("explains gitignored glob exclusions and the flag that includes them", async () => {
    const root = makeWorkspace();
    const target = path.join(
      root,
      "node_modules",
      "lucide-react",
      "dist",
      "search.d.ts"
    );
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(path.join(root, ".gitignore"), "node_modules/\n");
    fs.writeFileSync(target, "export type Search = unknown;\n");

    const excluded = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "**/search.d.ts" },
      filesystemNotEnforced
    );
    const excludedText = excluded.content[0]?.text ?? "";
    expect(excludedText).toContain("MATCHES: 0");
    expect(excludedText).toMatch(/NOTE: .*\.gitignore.*node_modules/);
    expect(excludedText).toContain("respect_gitignore=false");

    const included = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "**/search.d.ts", respect_gitignore: false },
      filesystemNotEnforced
    );
    expect(included.content[0]?.text).toContain(
      "node_modules/lucide-react/dist/search.d.ts"
    );
  });

  it("reports gitignored and hidden entries omitted from a directory listing", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, ".gitignore"), "dist/\n");
    fs.mkdirSync(path.join(root, "dist"));
    fs.writeFileSync(path.join(root, ".env"), "TOKEN=1\n");
    fs.writeFileSync(path.join(root, "visible.ts"), "export {};\n");

    const result = await handleFilesystemToolCall(
      "fs_list_dir",
      { path: "." },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toMatch(/NOTE: .*excluded by \.gitignore.*hidden path/);
    expect(text).toContain("respect_gitignore=false");
    expect(text).toContain("include_hidden=true");
  });

  it("reports a list entry cap only when another includable entry exists", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, "a.ts"), "export {};\n");
    fs.writeFileSync(path.join(root, "b.ts"), "export {};\n");

    const exact = await handleFilesystemToolCall(
      "fs_list_dir",
      {
        path: ".",
        max_entries: 2,
        include_hidden: true,
        respect_gitignore: false,
      },
      filesystemNotEnforced
    );
    expect(exact.content[0]?.text).not.toContain("entry cap reached");

    fs.writeFileSync(path.join(root, "c.ts"), "export {};\n");
    const capped = await handleFilesystemToolCall(
      "fs_list_dir",
      {
        path: ".",
        max_entries: 2,
        include_hidden: true,
        respect_gitignore: false,
      },
      filesystemNotEnforced
    );
    expect(capped.content[0]?.text).toContain(
      "NOTE: entry cap reached (max_entries=2); more entries exist."
    );
  });

  it("reports when fs_glob reaches max_results", async () => {
    const root = makeWorkspace();
    for (const name of ["a.ts", "b.ts", "c.ts"]) {
      fs.writeFileSync(path.join(root, name), "export {};\n");
    }

    const result = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "**/*.ts", max_results: 2 },
      filesystemNotEnforced
    );

    expect(result.content[0]?.text).toContain(
      "NOTE: result cap reached (max_results=2); more matches exist."
    );
  });

  it("does not claim more glob matches when results exactly fill the cap", async () => {
    const root = makeWorkspace();
    for (const name of ["a.ts", "b.ts"]) {
      fs.writeFileSync(path.join(root, name), "export {};\n");
    }

    const result = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "**/*.ts", max_results: 2, include_hidden: true },
      filesystemNotEnforced
    );

    expect(result.content[0]?.text).not.toContain("result cap reached");
  });

  it("reports files skipped by fs_grep for exceeding max_file_bytes", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, "large.ts"), "needle".repeat(100));

    const result = await handleFilesystemToolCall(
      "fs_grep",
      { pattern: "needle", max_file_bytes: 20 },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toMatch(/NOTE: .*max_file_bytes=20.*large\.ts/);
  });

  it("reports symlinks skipped during recursive traversal", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, "target.ts"), "export {};\n");
    fs.symlinkSync(path.join(root, "target.ts"), path.join(root, "linked.ts"));

    const result = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "**/*.ts" },
      filesystemNotEnforced
    );

    expect(result.content[0]?.text).toMatch(/NOTE: .*symlink skipped/);
  });

  it("reports directories not descended at max_depth", async () => {
    const root = makeWorkspace();
    const target = path.join(root, "nested", "deeper", "target.ts");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "export {};\n");

    const result = await handleFilesystemToolCall(
      "fs_glob",
      { pattern: "**/*.ts", max_depth: 0 },
      filesystemNotEnforced
    );

    expect(result.content[0]?.text).toContain(
      "1 directory not descended at max_depth=0"
    );
  });

  it("returns grep context with distinct prefixes and group separators", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(
      path.join(root, "sample.ts"),
      ["before one", "needle one", "after one", "gap", "before two", "needle two", "after two"].join("\n")
    );

    const result = await handleFilesystemToolCall(
      "fs_grep",
      { pattern: "needle", context_lines: 1 },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("sample.ts-1- before one");
    expect(text).toContain("sample.ts:2: needle one");
    expect(text).toContain("sample.ts-3- after one");
    expect(text).toMatch(/sample\.ts-3- after one\n--\nsample\.ts-5- before two/);
  });

  it("deduplicates overlapping grep context windows", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(
      path.join(root, "sample.ts"),
      ["before", "needle one", "shared", "needle two", "after"].join("\n")
    );

    const result = await handleFilesystemToolCall(
      "fs_grep",
      { pattern: "needle", context_lines: 2 },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text.match(/sample\.ts-3- shared/g)).toHaveLength(1);
    expect(text).not.toContain("\n--\n");
  });

  it("returns one bare path per matching file in files_with_matches mode", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, "a.ts"), "needle\nneedle\n");
    fs.writeFileSync(path.join(root, "b.ts"), "needle\n");

    const result = await handleFilesystemToolCall(
      "fs_grep",
      { pattern: "needle", output_mode: "files_with_matches" },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("OUTPUT_MODE: files_with_matches");
    expect(text).toContain("MATCHES: 2");
    expect(text).toMatch(/\na\.ts\nb\.ts$/);
    expect(text).not.toContain("a.ts:1:");
  });

  it("returns per-file match counts in count mode", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, "a.ts"), "needle\nneedle\n");
    fs.writeFileSync(path.join(root, "b.ts"), "needle\n");

    const result = await handleFilesystemToolCall(
      "fs_grep",
      { pattern: "needle", output_mode: "count" },
      filesystemNotEnforced
    );

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("OUTPUT_MODE: count");
    expect(text).toContain("MATCHES: 2");
    expect(text).toContain("a.ts:2");
    expect(text).toContain("b.ts:1");
  });

  it("reports a grep result cap only after observing an extra match", async () => {
    const root = makeWorkspace();
    fs.writeFileSync(path.join(root, "a.ts"), "needle\nneedle\n");

    const exact = await handleFilesystemToolCall(
      "fs_grep",
      {
        pattern: "needle",
        max_results: 2,
        include_hidden: true,
        respect_gitignore: false,
      },
      filesystemNotEnforced
    );
    expect(exact.content[0]?.text).not.toContain("result cap reached");

    fs.writeFileSync(path.join(root, "b.ts"), "needle\n");
    const capped = await handleFilesystemToolCall(
      "fs_grep",
      {
        pattern: "needle",
        max_results: 2,
        include_hidden: true,
        respect_gitignore: false,
      },
      filesystemNotEnforced
    );
    expect(capped.content[0]?.text).toContain(
      "NOTE: result cap reached (max_results=2); more matches exist."
    );
  });

  it("formats tool errors with the relative path resolution root", () => {
    const root = makeWorkspace();
    const result = formatFilesystemToolError(new Error("boom"));
    const text = result.content[0]?.text ?? "";
    expect(text).toContain("ERROR: boom");
    expect(text).toContain("Path resolution:");
    expect(text).toContain(`- ${root}`);
    expect(result.isError).toBe(true);
  });

  it("enforces configured roots without implicitly allowing cwd", async () => {
    const cwd = makeWorkspace();
    const allowedRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-allowed-"))
    );
    tempDirs.push(allowedRoot);
    process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([allowedRoot]);
    const insideFile = path.join(allowedRoot, "inside.txt");
    const cwdFile = path.join(cwd, "cwd.txt");
    fs.writeFileSync(insideFile, "allowed context\n");
    fs.writeFileSync(cwdFile, "implicit cwd must be denied\n");

    const inside = await handleFilesystemToolCall(
      "fs_read_file",
      { path: insideFile },
      { filesystemEnforced: true }
    );
    expect(inside.content[0]?.text).toContain("allowed context");

    await expect(
      handleFilesystemToolCall(
        "fs_list_dir",
        { path: allowedRoot },
        { filesystemEnforced: true }
      )
    ).resolves.toHaveProperty("content");
    await expect(
      handleFilesystemToolCall(
        "fs_read_file",
        { path: cwdFile },
        { filesystemEnforced: true }
      )
    ).rejects.toThrow("outside the allowed filesystem roots");
    await expect(
      handleFilesystemToolCall(
        "fs_read_file",
        { path: path.join(allowedRoot, "..", path.basename(cwd), "cwd.txt") },
        { filesystemEnforced: true }
      )
    ).rejects.toThrow("outside the allowed filesystem roots");
  });

  it("rejects absolute and symlink escapes in enforced mode", async () => {
    makeWorkspace();
    const allowedRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-allowed-"))
    );
    const outsideRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-outside-"))
    );
    tempDirs.push(allowedRoot, outsideRoot);
    process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([allowedRoot]);
    const outsideFile = path.join(outsideRoot, "secret.txt");
    fs.writeFileSync(outsideFile, "secret\n");
    const symlinkPath = path.join(allowedRoot, "escape.txt");
    fs.symlinkSync(outsideFile, symlinkPath);

    for (const target of [outsideFile, symlinkPath]) {
      await expect(
        handleFilesystemToolCall(
          "fs_read_file",
          { path: target },
          { filesystemEnforced: true }
        )
      ).rejects.toThrow("outside the allowed filesystem roots");
    }
  });

  it("denies every filesystem tool when enforced roots are empty", async () => {
    makeWorkspace();
    delete process.env.RALPHX_FILESYSTEM_READ_ROOTS;

    const calls: Array<[string, Record<string, unknown>]> = [
      ["fs_read_file", { path: "README.md" }],
      ["fs_list_dir", { path: "." }],
      ["fs_grep", { pattern: "needle", base_path: "." }],
      ["fs_glob", { pattern: "**/*", base_path: "." }],
    ];

    for (const [name, args] of calls) {
      await expect(
        handleFilesystemToolCall(name, args, { filesystemEnforced: true })
      ).rejects.toThrow("outside the allowed filesystem roots");
    }
  });

  it("preserves normal not-found errors only for missing paths inside an enforced root", async () => {
    makeWorkspace();
    const allowedRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-allowed-"))
    );
    const outsideRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-outside-"))
    );
    tempDirs.push(allowedRoot, outsideRoot);
    process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([allowedRoot]);

    await expect(
      handleFilesystemToolCall(
        "fs_read_file",
        { path: path.join(allowedRoot, "missing", "file.txt") },
        { filesystemEnforced: true }
      )
    ).rejects.toMatchObject({ code: "ENOENT" });
    await expect(
      handleFilesystemToolCall(
        "fs_read_file",
        { path: path.join(outsideRoot, "missing.txt") },
        { filesystemEnforced: true }
      )
    ).rejects.toThrow("outside the allowed filesystem roots");
  });
});
