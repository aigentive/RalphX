import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { readLineWindow } from "../filesystem/read-window.js";

describe("readLineWindow", () => {
  const tempDirs: string[] = [];

  afterEach(() => {
    for (const dir of tempDirs.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  function writeFile(content: string | Buffer): string {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-read-window-"));
    tempDirs.push(root);
    const target = path.join(root, "fixture.txt");
    fs.writeFileSync(target, content);
    return target;
  }

  function writeNumberedFile(lineCount: number, lineWidth = 24): string {
    const lines = Array.from({ length: lineCount }, (_, index) => {
      const prefix = `line ${index + 1} `;
      return `${prefix}${"x".repeat(Math.max(0, lineWidth - prefix.length))}`;
    });
    return writeFile(lines.join("\n"));
  }

  it("counts all lines when the file is larger than the returned byte window", async () => {
    const target = writeNumberedFile(3200, 40);

    const window = await readLineWindow(target, {
      startLine: 1,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.totalLines).toBe(3200);
    expect(window.totalLinesExact).toBe(true);
  });

  it("returns a narrow window at a high line offset", async () => {
    const target = writeNumberedFile(3200);

    const window = await readLineWindow(target, {
      startLine: 2699,
      maxLines: 60,
      maxBytes: 65_536,
    });

    expect(window.lines).toHaveLength(60);
    expect(window.lines[0]).toMatch(/^line 2699 /);
    expect(window.lines[59]).toMatch(/^line 2758 /);
  });

  it("stops at an explicit inclusive end line", async () => {
    const target = writeNumberedFile(3200);

    const window = await readLineWindow(target, {
      startLine: 2699,
      endLine: 2730,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.lines).toHaveLength(32);
    expect(window.stoppedBy).toBe("end_line");
  });

  it("reports a start line past EOF as out of range", async () => {
    const target = writeNumberedFile(3200);

    const window = await readLineWindow(target, {
      startLine: 5000,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.lines).toEqual([]);
    expect(window.stoppedBy).toBe("out_of_range");
    expect(window.totalLines).toBe(3200);
  });

  it("reports an empty file without a phantom line", async () => {
    const target = writeFile("");

    const window = await readLineWindow(target, {
      startLine: 1,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.totalLines).toBe(0);
    expect(window.lines).toEqual([]);
    expect(window.stoppedBy).toBe("out_of_range");
  });

  it("does not count a trailing newline as an empty content line", async () => {
    const target = writeFile("one\ntwo\nthree\n");

    const window = await readLineWindow(target, {
      startLine: 1,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.totalLines).toBe(3);
    expect(window.lines).toEqual(["one", "two", "three"]);
  });

  it("counts and returns a final partial line", async () => {
    const target = writeFile("one\ntwo\nthree");

    const window = await readLineWindow(target, {
      startLine: 1,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.totalLines).toBe(3);
    expect(window.lines).toEqual(["one", "two", "three"]);
  });

  it("names max_lines when it prevents reaching the requested end", async () => {
    const target = writeNumberedFile(600);

    const window = await readLineWindow(target, {
      startLine: 1,
      endLine: 500,
      maxLines: 10,
      maxBytes: 65_536,
    });

    expect(window.lines).toHaveLength(10);
    expect(window.stoppedBy).toBe("max_lines");
  });

  it("prefers an explicit end_line reached before max_lines", async () => {
    const target = writeNumberedFile(100);

    const window = await readLineWindow(target, {
      startLine: 1,
      endLine: 20,
      maxLines: 2000,
      maxBytes: 65_536,
    });

    expect(window.lines).toHaveLength(20);
    expect(window.stoppedBy).toBe("end_line");
  });

  it("stops the returned window at max_bytes while continuing the line count", async () => {
    const target = writeNumberedFile(100, 20);

    const window = await readLineWindow(target, {
      startLine: 1,
      endLine: 80,
      maxLines: 100,
      maxBytes: 64,
    });

    expect(window.stoppedBy).toBe("max_bytes");
    expect(window.windowEnd).toBeLessThan(80);
    expect(window.totalLines).toBe(100);
  });

  it("returns one oversized line instead of an empty window", async () => {
    const target = writeFile(`${"x".repeat(200)}\nsecond`);

    const window = await readLineWindow(target, {
      startLine: 1,
      maxLines: 20,
      maxBytes: 32,
    });

    expect(window.lines).toEqual(["x".repeat(200)]);
    expect(window.stoppedBy).toBe("max_bytes");
  });

  it("preserves UTF-8 characters split across a read chunk boundary", async () => {
    const target = writeFile(`${"a".repeat(65_535)}é\nhéllo — 世界`);

    const window = await readLineWindow(target, {
      startLine: 1,
      maxLines: 2,
      maxBytes: 131_072,
    });

    expect(window.lines).toHaveLength(2);
    expect(window.lines.join("\n")).not.toContain("�");
    expect(window.lines[0]).toMatch(/é$/);
    expect(window.lines[1]).toBe("héllo — 世界");
  });

  it("rejects binary files", async () => {
    const target = writeFile(Buffer.from([0x61, 0x00, 0x62]));

    await expect(
      readLineWindow(target, {
        startLine: 1,
        maxLines: 20,
        maxBytes: 65_536,
      })
    ).rejects.toThrow(/appears to be binary/);
  });
});
