import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  formatFilesystemToolError,
  handleFilesystemToolCall,
} from "../filesystem-tools.js";

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

  it("reads a relative file within the allowed root", async () => {
    const root = makeWorkspace();
    const target = path.join(root, "src", "sample.ts");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "line one\nline two\nline three\n");

    const result = await handleFilesystemToolCall("fs_read_file", {
      path: "src/sample.ts",
      start_line: 2,
      end_line: 3,
    });

    const text = result.content[0]?.text ?? "";
    expect(text).toContain(`FILE: ${target}`);
    expect(text).toContain("LINES: 2-3/4");
    expect(text).toContain("2| line two");
    expect(text).toContain("3| line three");
  });

  it("reads and lists absolute paths from a configured extra read root", async () => {
    makeWorkspace();
    const projectRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-project-"))
    );
    tempDirs.push(projectRoot);
    process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([projectRoot]);

    const target = path.join(projectRoot, ".artifacts", "specs", "ralphx-cli", "tracker.md");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, "# CLI tracker\n\nreadable from project checkout\n");

    const readResult = await handleFilesystemToolCall("fs_read_file", {
      path: target,
    });
    const readText = readResult.content[0]?.text ?? "";
    expect(readText).toContain(`FILE: ${target}`);
    expect(readText).toContain("readable from project checkout");

    const listResult = await handleFilesystemToolCall("fs_list_dir", {
      path: path.dirname(target),
      include_hidden: true,
    });
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

    const result = await handleFilesystemToolCall("fs_list_dir", {
      path: ".",
    });

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("DIR  src/");
    expect(text).toContain("FILE visible.ts");
    expect(text).not.toContain("dist/");
    expect(text).not.toContain("secret.log");
    expect(text).not.toContain(".env");
  });

  it("rejects traversal outside the allowed root", async () => {
    const root = makeWorkspace();
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-outside-"));
    tempDirs.push(outsideDir);
    const outsideFile = path.join(outsideDir, "secret.txt");
    fs.writeFileSync(outsideFile, "secret\n");

    await expect(
      handleFilesystemToolCall("fs_read_file", {
        path: path.relative(root, outsideFile),
      })
    ).rejects.toThrow("outside the allowed filesystem roots");
  });

  it("rejects symlinked file paths that escape the allowed root", async () => {
    const root = makeWorkspace();
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-link-outside-"));
    tempDirs.push(outsideDir);
    const outsideFile = path.join(outsideDir, "secret.txt");
    fs.writeFileSync(outsideFile, "secret\n");

    const symlinkPath = path.join(root, "src", "escape.txt");
    fs.mkdirSync(path.dirname(symlinkPath), { recursive: true });
    fs.symlinkSync(outsideFile, symlinkPath);

    await expect(
      handleFilesystemToolCall("fs_read_file", {
        path: "src/escape.txt",
      })
    ).rejects.toThrow("outside the allowed filesystem roots");
  });

  it("rejects symlinked base paths that escape the allowed root", async () => {
    const root = makeWorkspace();
    const outsideDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-base-outside-"));
    tempDirs.push(outsideDir);
    fs.writeFileSync(path.join(outsideDir, "secret.ts"), "export const secret = true;\n");

    const symlinkPath = path.join(root, "linked");
    fs.symlinkSync(outsideDir, symlinkPath);

    await expect(
      handleFilesystemToolCall("fs_glob", {
        base_path: "linked",
        pattern: "**/*.ts",
      })
    ).rejects.toThrow("outside the allowed filesystem roots");
  });

  it("greps and globs absolute base paths from a configured extra read root", async () => {
    makeWorkspace();
    const projectRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-search-project-"))
    );
    tempDirs.push(projectRoot);
    process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([projectRoot]);
    const sourceFile = path.join(projectRoot, "service", "index.ts");
    fs.mkdirSync(path.dirname(sourceFile), { recursive: true });
    fs.writeFileSync(sourceFile, "export const externalNeedle = true;\n");

    const grepResult = await handleFilesystemToolCall("fs_grep", {
      pattern: "externalNeedle",
      base_path: projectRoot,
      file_pattern: "**/*.ts",
    });
    const grepText = grepResult.content[0]?.text ?? "";
    expect(grepText).toContain(`ROOT: ${projectRoot}`);
    expect(grepText).toContain("service/index.ts:1: export const externalNeedle = true;");

    const globResult = await handleFilesystemToolCall("fs_glob", {
      base_path: projectRoot,
      pattern: "**/*.ts",
    });
    const globText = globResult.content[0]?.text ?? "";
    expect(globText).toContain(`ROOT: ${projectRoot}`);
    expect(globText).toContain("service/index.ts");
  });

  it("rejects unconfigured absolute base paths outside the allowed roots", async () => {
    makeWorkspace();
    const projectRoot = fs.realpathSync(
      fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-fs-unconfigured-project-"))
    );
    tempDirs.push(projectRoot);
    const sourceFile = path.join(projectRoot, "service", "index.ts");
    fs.mkdirSync(path.dirname(sourceFile), { recursive: true });
    fs.writeFileSync(sourceFile, "export const externalNeedle = true;\n");

    await expect(
      handleFilesystemToolCall("fs_grep", {
        pattern: "externalNeedle",
        base_path: projectRoot,
        file_pattern: "**/*.ts",
      })
    ).rejects.toThrow("outside the allowed filesystem roots");

    await expect(
      handleFilesystemToolCall("fs_glob", {
        base_path: projectRoot,
        pattern: "**/*.ts",
      })
    ).rejects.toThrow("outside the allowed filesystem roots");
  });

  it("greps within the allowed root using a file pattern", async () => {
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

    const result = await handleFilesystemToolCall("fs_grep", {
      pattern: "delegate_start",
      base_path: ".",
      file_pattern: "**/*.rs",
    });

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("FILE_PATTERN: **/*.rs");
    expect(text).toContain("src-tauri/src/main.rs:2:     println!(\"delegate_start\");");
    expect(text).not.toContain("README.md");
    expect(text).not.toContain("ignored.rs");
  });

  it("globs within the allowed root", async () => {
    const root = makeWorkspace();
    const first = path.join(root, "agents", "one", "codex", "prompt.md");
    const second = path.join(root, "agents", "two", "codex", "prompt.md");
    fs.mkdirSync(path.dirname(first), { recursive: true });
    fs.mkdirSync(path.dirname(second), { recursive: true });
    fs.writeFileSync(first, "# one\n");
    fs.writeFileSync(second, "# two\n");
    fs.writeFileSync(path.join(root, ".gitignore"), "agents/two/\n");

    const result = await handleFilesystemToolCall("fs_glob", {
      pattern: "agents/**/codex/*.md",
    });

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

    const result = await handleFilesystemToolCall("fs_glob", {
      base_path: "src",
      pattern: "**/*.ts",
      max_depth: 0,
    });

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("one.ts");
    expect(text).not.toContain("nested/two.ts");
  });

  it("caps file reads without loading the entire file into the response", async () => {
    const root = makeWorkspace();
    const target = path.join(root, "src", "large.ts");
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, `${"x".repeat(4096)}\n${"y".repeat(4096)}\n`);

    const result = await handleFilesystemToolCall("fs_read_file", {
      path: "src/large.ts",
      max_bytes: 128,
    });

    const text = result.content[0]?.text ?? "";
    expect(text).toContain("TRUNCATED: true");
  });

  it("formats tool errors with the allowed root", () => {
    const root = makeWorkspace();
    const result = formatFilesystemToolError(new Error("boom"));
    const text = result.content[0]?.text ?? "";
    expect(text).toContain("ERROR: boom");
    expect(text).toContain("Allowed filesystem roots:");
    expect(text).toContain(`- ${root}`);
    expect(result.isError).toBe(true);
  });
});
