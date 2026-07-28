import { describe, expect, it } from "vitest";

import goldens from "./qr-encode.goldens.json";
import { encodeQrMatrix, qrMatrixToSvg } from "./qr-encode";

/**
 * The goldens were produced once by an independent reference encoder
 * (node-qrcode 1.5.4, byte mode, ECC level M) and committed as fixtures. The
 * reference is NOT a dependency of this project — it only pinned these vectors.
 *
 * A QR that renders but does not scan is worse than no QR, and nothing in CI can
 * point a camera at one, so exact-matrix equality against an independent encoder
 * is the correctness proof. Any drift in version selection, block interleaving,
 * Reed-Solomon, mask choice, or function-pattern placement changes the matrix.
 */
describe("encodeQrMatrix — golden vectors from an independent encoder", () => {
  for (const golden of goldens) {
    it(`reproduces ${golden.name} exactly (version ${golden.version}, mask ${golden.maskPattern})`, () => {
      const matrix = encodeQrMatrix(golden.data);

      expect(matrix.version).toBe(golden.version);
      expect(matrix.maskPattern).toBe(golden.maskPattern);
      expect(matrix.size).toBe(golden.size);

      const rows = matrix.modules.map((row) =>
        row.map((on) => (on ? "1" : "0")).join(""),
      );
      expect(rows).toEqual(golden.rows);
    });
  }

  it("covers version 1, a mid version with alignment patterns, and a version >= 7 with version-info blocks", () => {
    const versions = goldens.map((g) => g.version);
    expect(versions).toContain(1);
    expect(Math.max(...versions)).toBeGreaterThanOrEqual(7);
  });
});

describe("encodeQrMatrix — structure", () => {
  it("places the three finder patterns", () => {
    const { modules, size } = encodeQrMatrix("ralphx://pair?host=h#code=rxp_A");
    // Finder centre is a 3x3 dark block at (3,3) offsets from each corner.
    for (const [oy, ox] of [
      [0, 0],
      [0, size - 7],
      [size - 7, 0],
    ]) {
      expect(modules[oy + 3][ox + 3]).toBe(true);
      // Separator ring inside the 7x7 is light at the 1-offset ring corner.
      expect(modules[oy + 1][ox + 1]).toBe(false);
    }
  });

  it("rejects payloads that do not fit the supported version range", () => {
    expect(() => encodeQrMatrix("x".repeat(10_000))).toThrow(/too long/i);
  });

  it("encodes non-ASCII payloads as UTF-8 bytes", () => {
    expect(() => encodeQrMatrix("café ☕")).not.toThrow();
  });
});

describe("qrMatrixToSvg", () => {
  const matrix = encodeQrMatrix("ralphx://pair?host=h#code=rxp_ABCD");

  it("renders literal black-on-white, never CSS variables (WKWebView + scanability)", () => {
    const svg = qrMatrixToSvg(matrix);
    expect(svg).toContain("#000000");
    expect(svg).toContain("#FFFFFF");
    expect(svg).not.toContain("var(--");
    expect(svg).not.toContain("currentColor");
  });

  it("includes the mandatory 4-module quiet zone in the viewBox", () => {
    const svg = qrMatrixToSvg(matrix);
    const expected = matrix.size + 8;
    expect(svg).toContain(`viewBox="0 0 ${expected} ${expected}"`);
  });

  it("scales to any rendered size without re-encoding", () => {
    const svg = qrMatrixToSvg(matrix);
    // No absolute pixel width/height baked in — the container sizes it.
    expect(svg).not.toMatch(/width="\d+px"/);
  });

  it("is stable for a fixed payload", () => {
    expect(qrMatrixToSvg(matrix)).toBe(
      qrMatrixToSvg(encodeQrMatrix("ralphx://pair?host=h#code=rxp_ABCD")),
    );
  });
});
