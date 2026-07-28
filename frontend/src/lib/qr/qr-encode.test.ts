import { describe, expect, it } from "vitest";

import goldens from "./qr-encode.goldens.json";
import {
  QR_QUIET_ZONE,
  encodeQrMatrix,
  qrMatrixToPath,
  qrSvgExtent,
} from "./qr-encode";

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

describe("qrMatrixToPath", () => {
  const matrix = encodeQrMatrix("ralphx://pair?host=h#code=rxp_ABCD");

  it("reserves the mandatory 4-module quiet zone around the symbol", () => {
    expect(QR_QUIET_ZONE).toBe(4);
    expect(qrSvgExtent(matrix)).toBe(matrix.size + 8);

    // Every coordinate is pushed in by the quiet zone, so nothing is drawn
    // against the edge of the drawing area.
    const coords = [...qrMatrixToPath(matrix).matchAll(/M(\d+) (\d+)/g)];
    expect(coords.length).toBeGreaterThan(0);
    for (const [, x, y] of coords) {
      expect(Number(x)).toBeGreaterThanOrEqual(QR_QUIET_ZONE);
      expect(Number(y)).toBeGreaterThanOrEqual(QR_QUIET_ZONE);
      expect(Number(x)).toBeLessThan(matrix.size + QR_QUIET_ZONE);
      expect(Number(y)).toBeLessThan(matrix.size + QR_QUIET_ZONE);
    }
  });

  it("merges horizontally adjacent modules into single runs", () => {
    const darkModules = matrix.modules.flat().filter(Boolean).length;
    const runs = qrMatrixToPath(matrix).match(/M/g)?.length ?? 0;

    // The top-left finder alone guarantees runs wider than one module.
    expect(runs).toBeGreaterThan(0);
    expect(runs).toBeLessThan(darkModules);
  });

  it("carries no colour of its own — the caller supplies literal fills", () => {
    const path = qrMatrixToPath(matrix);
    expect(path).not.toContain("var(--");
    expect(path).not.toContain("currentColor");
    expect(path).toMatch(/^[Mhvz0-9 ,.-]*$/);
  });

  it("is stable for a fixed payload", () => {
    expect(qrMatrixToPath(matrix)).toBe(
      qrMatrixToPath(encodeQrMatrix("ralphx://pair?host=h#code=rxp_ABCD")),
    );
  });

  it("shifts with a custom quiet zone", () => {
    const tight = qrMatrixToPath(matrix, 0);
    const padded = qrMatrixToPath(matrix, 4);
    expect(tight).not.toBe(padded);
    expect(qrSvgExtent(matrix, 0)).toBe(matrix.size);
  });
});
