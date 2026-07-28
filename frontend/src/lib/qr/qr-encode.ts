/**
 * Minimal QR Code encoder — byte mode, error correction level M, versions 1–10.
 *
 * WHY THIS EXISTS INSTEAD OF A DEPENDENCY
 * The only QR we render is the device-pairing URL (§4.2/§5.4): one short ASCII
 * payload, one ECC level, one output format (SVG). A general QR package carries
 * canvas/terminal/PNG renderers, Kanji tables, and multi-segment mode-splitting
 * that this surface never reaches. This module is the ~1% of the format that the
 * pairing card actually needs, with no runtime dependency and no bundle growth.
 *
 * CORRECTNESS
 * A QR that renders but does not scan is worse than no QR, and no CI job can point
 * a camera at one. So `qr-encode.test.ts` asserts EXACT matrix equality against
 * golden vectors produced once by an independent reference encoder (node-qrcode
 * 1.5.4, byte mode, level M). Those vectors cover version 1, version 5 (alignment
 * patterns), and version 7 (version-information blocks). Any drift in Reed-Solomon,
 * block interleaving, function-pattern placement, or mask selection moves the
 * matrix and fails the suite.
 *
 * Mask selection follows the same penalty formulas as that reference. Mask choice
 * is a quality heuristic, not a decodability requirement — every mask yields a
 * readable symbol — but matching it lets the goldens compare whole matrices rather
 * than only the parts the spec pins down.
 *
 * SCOPE LIMITS (deliberate)
 * - Byte mode only. Alphanumeric mode would shrink some payloads, but pairing URLs
 *   contain lowercase and `%`, so they never qualify.
 * - Level M only (~15% recovery), the usual choice for URLs.
 * - Versions 1–10 (up to 216 data codewords). A pairing URL is ~80 bytes; the cap
 *   is an explicit throw rather than silent truncation.
 */

/** Total codewords (data + error correction) per version, index = version - 1. */
const TOTAL_CODEWORDS = [26, 44, 70, 100, 134, 172, 196, 242, 292, 346];
/** Error-correction block count at level M, index = version - 1. */
const EC_BLOCKS_M = [1, 1, 1, 2, 2, 4, 4, 4, 5, 5];
/** Total error-correction codewords at level M, index = version - 1. */
const EC_CODEWORDS_M = [10, 16, 26, 36, 48, 64, 72, 88, 110, 130];

const MAX_VERSION = 10;
const BYTE_MODE_INDICATOR = 0b0100;
/** Level M is bit pattern 00 in the format-information field. */
const EC_LEVEL_BITS_M = 0;
const PAD_BYTES = [0xec, 0x11];

const G15 =
  (1 << 10) | (1 << 8) | (1 << 5) | (1 << 4) | (1 << 2) | (1 << 1) | (1 << 0);
const G15_MASK = (1 << 14) | (1 << 12) | (1 << 10) | (1 << 4) | (1 << 1);
const G18 =
  (1 << 12) |
  (1 << 11) |
  (1 << 10) |
  (1 << 9) |
  (1 << 8) |
  (1 << 5) |
  (1 << 2) |
  (1 << 0);

export interface QrMatrix {
  /** Width and height in modules, excluding the quiet zone. */
  size: number;
  version: number;
  maskPattern: number;
  /** Row-major; `true` is a dark module. */
  modules: boolean[][];
}

// ---------------------------------------------------------------------------
// GF(256) arithmetic for Reed-Solomon
// ---------------------------------------------------------------------------

const EXP_TABLE = new Uint8Array(512);
const LOG_TABLE = new Uint8Array(256);
{
  let x = 1;
  for (let i = 0; i < 255; i++) {
    EXP_TABLE[i] = x;
    LOG_TABLE[x] = i;
    x <<= 1;
    if (x & 0x100) {
      x ^= 0x11d; // primitive polynomial for QR's GF(256)
    }
  }
  for (let i = 255; i < 512; i++) {
    EXP_TABLE[i] = EXP_TABLE[i - 255]!;
  }
}

function gfMul(a: number, b: number): number {
  if (a === 0 || b === 0) {
    return 0;
  }
  return EXP_TABLE[LOG_TABLE[a]! + LOG_TABLE[b]!]!;
}

/** Generator polynomial for `degree` error-correction codewords. */
function generatorPoly(degree: number): Uint8Array {
  let poly = new Uint8Array([1]);
  for (let d = 0; d < degree; d++) {
    const next = new Uint8Array(poly.length + 1);
    for (let i = 0; i < poly.length; i++) {
      next[i] = next[i]! ^ poly[i]!;
      next[i + 1] = next[i + 1]! ^ gfMul(poly[i]!, EXP_TABLE[d]!);
    }
    poly = next;
  }
  return poly;
}

/** Polynomial-division remainder — the error-correction codewords for one block. */
function reedSolomonEncode(data: Uint8Array, ecCount: number): Uint8Array {
  const gen = generatorPoly(ecCount);
  const remainder = new Uint8Array(ecCount);

  for (const byte of data) {
    const factor = byte ^ remainder[0]!;
    remainder.copyWithin(0, 1);
    remainder[ecCount - 1] = 0;
    for (let i = 0; i < ecCount; i++) {
      remainder[i] = remainder[i]! ^ gfMul(gen[i + 1]!, factor);
    }
  }

  return remainder;
}

// ---------------------------------------------------------------------------
// BCH-encoded format and version information
// ---------------------------------------------------------------------------

function bchDigit(value: number): number {
  let digit = 0;
  let v = value;
  while (v !== 0) {
    digit++;
    v >>>= 1;
  }
  return digit;
}

const G15_BCH = bchDigit(G15);
const G18_BCH = bchDigit(G18);

/** 15-bit BCH(15,5) format information for level M and the given mask. */
function formatInfoBits(maskPattern: number): number {
  const data = (EC_LEVEL_BITS_M << 3) | maskPattern;
  let d = data << 10;
  while (bchDigit(d) - G15_BCH >= 0) {
    d ^= G15 << (bchDigit(d) - G15_BCH);
  }
  // The XOR mask guarantees no level/mask combination encodes to all zeroes.
  return ((data << 10) | d) ^ G15_MASK;
}

/** 18-bit BCH(18,6) version information; only versions >= 7 carry it. */
function versionInfoBits(version: number): number {
  let d = version << 12;
  while (bchDigit(d) - G18_BCH >= 0) {
    d ^= G18 << (bchDigit(d) - G18_BCH);
  }
  return (version << 12) | d;
}

// ---------------------------------------------------------------------------
// Version geometry
// ---------------------------------------------------------------------------

function symbolSize(version: number): number {
  return version * 4 + 17;
}

/** Byte-mode character-count indicator width. Widens at version 10. */
function charCountBits(version: number): number {
  return version < 10 ? 8 : 16;
}

/** Reads a per-version table, rejecting versions outside the supported range. */
function versionTable(table: readonly number[], version: number): number {
  const value = table[version - 1];
  if (value === undefined) {
    throw new Error(`Unsupported QR version: ${version}`);
  }
  return value;
}

function totalCodewords(version: number): number {
  return versionTable(TOTAL_CODEWORDS, version);
}

function ecCodewords(version: number): number {
  return versionTable(EC_CODEWORDS_M, version);
}

function ecBlockCount(version: number): number {
  return versionTable(EC_BLOCKS_M, version);
}

function dataCodewords(version: number): number {
  return totalCodewords(version) - ecCodewords(version);
}

/** Payload capacity in bytes, after the mode and character-count header. */
function byteCapacity(version: number): number {
  const availableBits = dataCodewords(version) * 8 - 4 - charCountBits(version);
  return Math.floor(availableBits / 8);
}

/** Alignment-pattern centre coordinates along one axis. */
function alignmentCoords(version: number): number[] {
  if (version === 1) {
    return [];
  }
  const posCount = Math.floor(version / 7) + 2;
  const size = symbolSize(version);
  const interval = Math.ceil((size - 13) / (2 * posCount - 2)) * 2;
  const positions = [size - 7];
  for (let i = 1; i < posCount - 1; i++) {
    positions.push(positions[i - 1]! - interval);
  }
  positions.push(6);
  return positions.reverse();
}

/** Alignment-pattern centres, minus the three that collide with finder patterns. */
function alignmentPositions(version: number): Array<[number, number]> {
  const coords = alignmentCoords(version);
  const positions: Array<[number, number]> = [];
  const last = coords.length - 1;
  for (let i = 0; i <= last; i++) {
    for (let j = 0; j <= last; j++) {
      const collidesWithFinder =
        (i === 0 && j === 0) ||
        (i === 0 && j === last) ||
        (i === last && j === 0);
      if (!collidesWithFinder) {
        positions.push([coords[i]!, coords[j]!]);
      }
    }
  }
  return positions;
}

// ---------------------------------------------------------------------------
// Data encoding
// ---------------------------------------------------------------------------

class BitBuffer {
  private readonly bits: number[] = [];

  put(value: number, length: number): void {
    for (let i = length - 1; i >= 0; i--) {
      this.bits.push((value >>> i) & 1);
    }
  }

  get length(): number {
    return this.bits.length;
  }

  toCodewords(count: number): Uint8Array {
    const out = new Uint8Array(count);
    for (let i = 0; i < this.bits.length; i++) {
      if (this.bits[i]) {
        const byteIndex = i >>> 3;
        out[byteIndex] = (out[byteIndex] ?? 0) | (0x80 >>> (i % 8));
      }
    }
    return out;
  }
}

/** Builds the padded data codewords: header, payload, terminator, pad bytes. */
function buildDataCodewords(bytes: Uint8Array, version: number): Uint8Array {
  const capacityCodewords = dataCodewords(version);
  const capacityBits = capacityCodewords * 8;

  const buffer = new BitBuffer();
  buffer.put(BYTE_MODE_INDICATOR, 4);
  buffer.put(bytes.length, charCountBits(version));
  for (const byte of bytes) {
    buffer.put(byte, 8);
  }

  // Terminator: up to four zero bits, fewer if the symbol is nearly full.
  const terminator = Math.min(4, capacityBits - buffer.length);
  buffer.put(0, terminator);
  // Pad to a byte boundary, then alternate the two specified pad codewords.
  buffer.put(0, (8 - (buffer.length % 8)) % 8);

  const codewords = buffer.toCodewords(capacityCodewords);
  let padIndex = 0;
  for (let i = buffer.length / 8; i < capacityCodewords; i++) {
    codewords[i] = PAD_BYTES[padIndex % 2]!;
    padIndex++;
  }
  return codewords;
}

/**
 * Splits data into blocks, appends per-block error correction, and interleaves
 * both — the spread that lets a scanner recover from a localised smudge.
 */
function interleaveCodewords(data: Uint8Array, version: number): Uint8Array {
  const symbolCodewords = totalCodewords(version);
  const blockCount = ecBlockCount(version);
  const dataTotal = dataCodewords(version);

  const blocksInGroup2 = symbolCodewords % blockCount;
  const blocksInGroup1 = blockCount - blocksInGroup2;
  const dataInGroup1 = Math.floor(dataTotal / blockCount);
  const ecCount = Math.floor(symbolCodewords / blockCount) - dataInGroup1;

  const dataBlocks: Uint8Array[] = [];
  const ecBlocks: Uint8Array[] = [];
  let offset = 0;
  let maxDataSize = 0;

  for (let b = 0; b < blockCount; b++) {
    const size = b < blocksInGroup1 ? dataInGroup1 : dataInGroup1 + 1;
    const block = data.slice(offset, offset + size);
    dataBlocks.push(block);
    ecBlocks.push(reedSolomonEncode(block, ecCount));
    offset += size;
    maxDataSize = Math.max(maxDataSize, size);
  }

  const out = new Uint8Array(symbolCodewords);
  let index = 0;
  for (let i = 0; i < maxDataSize; i++) {
    for (let b = 0; b < blockCount; b++) {
      if (i < dataBlocks[b]!.length) {
        out[index++] = dataBlocks[b]![i]!;
      }
    }
  }
  for (let i = 0; i < ecCount; i++) {
    for (let b = 0; b < blockCount; b++) {
      out[index++] = ecBlocks[b]![i]!;
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Matrix assembly
// ---------------------------------------------------------------------------

/** Modules plus a parallel "reserved" plane marking function patterns. */
class ModuleGrid {
  readonly size: number;
  readonly dark: Uint8Array;
  readonly reserved: Uint8Array;

  constructor(size: number) {
    this.size = size;
    this.dark = new Uint8Array(size * size);
    this.reserved = new Uint8Array(size * size);
  }

  private index(row: number, col: number): number {
    return row * this.size + col;
  }

  get(row: number, col: number): number {
    return this.dark[this.index(row, col)]!;
  }

  set(row: number, col: number, dark: boolean, reserved = false): void {
    const i = this.index(row, col);
    this.dark[i] = dark ? 1 : 0;
    if (reserved) {
      this.reserved[i] = 1;
    }
  }

  isReserved(row: number, col: number): boolean {
    return this.reserved[this.index(row, col)] === 1;
  }

  xor(row: number, col: number, value: boolean): void {
    const i = this.index(row, col);
    this.dark[i] = this.dark[i]! ^ (value ? 1 : 0);
  }
}

function placeFinderPatterns(grid: ModuleGrid): void {
  const size = grid.size;
  const origins: Array<[number, number]> = [
    [0, 0],
    [0, size - 7],
    [size - 7, 0],
  ];

  for (const [row, col] of origins) {
    // -1..7 covers the 7x7 pattern plus its light separator ring.
    for (let r = -1; r <= 7; r++) {
      if (row + r < 0 || row + r >= size) {
        continue;
      }
      for (let c = -1; c <= 7; c++) {
        if (col + c < 0 || col + c >= size) {
          continue;
        }
        const isRing =
          (r >= 0 && r <= 6 && (c === 0 || c === 6)) ||
          (c >= 0 && c <= 6 && (r === 0 || r === 6));
        const isCore = r >= 2 && r <= 4 && c >= 2 && c <= 4;
        grid.set(row + r, col + c, isRing || isCore, true);
      }
    }
  }
}

function placeTimingPatterns(grid: ModuleGrid): void {
  for (let i = 8; i < grid.size - 8; i++) {
    const dark = i % 2 === 0;
    grid.set(i, 6, dark, true);
    grid.set(6, i, dark, true);
  }
}

function placeAlignmentPatterns(grid: ModuleGrid, version: number): void {
  for (const [row, col] of alignmentPositions(version)) {
    for (let r = -2; r <= 2; r++) {
      for (let c = -2; c <= 2; c++) {
        const isEdge = r === -2 || r === 2 || c === -2 || c === 2;
        const isCentre = r === 0 && c === 0;
        grid.set(row + r, col + c, isEdge || isCentre, true);
      }
    }
  }
}

function placeVersionInfo(grid: ModuleGrid, version: number): void {
  if (version < 7) {
    return;
  }
  const size = grid.size;
  const bits = versionInfoBits(version);
  for (let i = 0; i < 18; i++) {
    const row = Math.floor(i / 3);
    const col = (i % 3) + size - 8 - 3;
    const dark = ((bits >> i) & 1) === 1;
    grid.set(row, col, dark, true);
    grid.set(col, row, dark, true);
  }
}

function placeFormatInfo(grid: ModuleGrid, maskPattern: number): void {
  const size = grid.size;
  const bits = formatInfoBits(maskPattern);

  for (let i = 0; i < 15; i++) {
    const dark = ((bits >> i) & 1) === 1;

    // Vertical strip beside the top-left finder, skipping the timing row.
    if (i < 6) {
      grid.set(i, 8, dark, true);
    } else if (i < 8) {
      grid.set(i + 1, 8, dark, true);
    } else {
      grid.set(size - 15 + i, 8, dark, true);
    }

    // Horizontal strip below the top-left / beside the top-right finder.
    if (i < 8) {
      grid.set(8, size - i - 1, dark, true);
    } else if (i < 9) {
      grid.set(8, 15 - i, dark, true);
    } else {
      grid.set(8, 14 - i, dark, true);
    }
  }

  // The always-dark module below the top-left format strip.
  grid.set(size - 8, 8, true, true);
}

/**
 * Zig-zags the codeword bits up and down two-column strips, right to left,
 * skipping the timing column and every reserved module. Any leftover remainder
 * bits stay light, which is what the spec asks for.
 */
function placeData(grid: ModuleGrid, codewords: Uint8Array): void {
  const size = grid.size;
  let inc = -1;
  let row = size - 1;
  let bitIndex = 7;
  let byteIndex = 0;

  for (let col = size - 1; col > 0; col -= 2) {
    if (col === 6) {
      col--; // the vertical timing pattern occupies column 6
    }

    for (;;) {
      for (let c = 0; c < 2; c++) {
        if (!grid.isReserved(row, col - c)) {
          let dark = false;
          if (byteIndex < codewords.length) {
            dark = ((codewords[byteIndex]! >>> bitIndex) & 1) === 1;
          }
          grid.set(row, col - c, dark);
          bitIndex--;
          if (bitIndex === -1) {
            byteIndex++;
            bitIndex = 7;
          }
        }
      }

      row += inc;
      if (row < 0 || row >= size) {
        row -= inc;
        inc = -inc;
        break;
      }
    }
  }
}

function maskAt(pattern: number, i: number, j: number): boolean {
  switch (pattern) {
    case 0:
      return (i + j) % 2 === 0;
    case 1:
      return i % 2 === 0;
    case 2:
      return j % 3 === 0;
    case 3:
      return (i + j) % 3 === 0;
    case 4:
      return (Math.floor(i / 2) + Math.floor(j / 3)) % 2 === 0;
    case 5:
      return ((i * j) % 2) + ((i * j) % 3) === 0;
    case 6:
      return (((i * j) % 2) + ((i * j) % 3)) % 2 === 0;
    default:
      return (((i * j) % 3) + ((i + j) % 2)) % 2 === 0;
  }
}

function applyMask(grid: ModuleGrid, pattern: number): void {
  for (let row = 0; row < grid.size; row++) {
    for (let col = 0; col < grid.size; col++) {
      if (!grid.isReserved(row, col)) {
        grid.xor(row, col, maskAt(pattern, row, col));
      }
    }
  }
}

/** Runs of five or more same-coloured modules in a row or column. */
function penaltyAdjacent(grid: ModuleGrid): number {
  const size = grid.size;
  let points = 0;

  for (let row = 0; row < size; row++) {
    let sameCountRow = 0;
    let sameCountCol = 0;
    let lastRow: number | null = null;
    let lastCol: number | null = null;

    for (let col = 0; col < size; col++) {
      const horizontal = grid.get(row, col);
      if (horizontal === lastRow) {
        sameCountRow++;
      } else {
        if (sameCountRow >= 5) {
          points += 3 + (sameCountRow - 5);
        }
        lastRow = horizontal;
        sameCountRow = 1;
      }

      const vertical = grid.get(col, row);
      if (vertical === lastCol) {
        sameCountCol++;
      } else {
        if (sameCountCol >= 5) {
          points += 3 + (sameCountCol - 5);
        }
        lastCol = vertical;
        sameCountCol = 1;
      }
    }

    if (sameCountRow >= 5) {
      points += 3 + (sameCountRow - 5);
    }
    if (sameCountCol >= 5) {
      points += 3 + (sameCountCol - 5);
    }
  }

  return points;
}

/** Solid 2x2 blocks. */
function penaltyBlocks(grid: ModuleGrid): number {
  let blocks = 0;
  for (let row = 0; row < grid.size - 1; row++) {
    for (let col = 0; col < grid.size - 1; col++) {
      const sum =
        grid.get(row, col) +
        grid.get(row, col + 1) +
        grid.get(row + 1, col) +
        grid.get(row + 1, col + 1);
      if (sum === 0 || sum === 4) {
        blocks++;
      }
    }
  }
  return blocks * 3;
}

/** Finder-like 1:1:3:1:1 runs, which a scanner could mistake for a finder. */
function penaltyFinderLike(grid: ModuleGrid): number {
  const size = grid.size;
  let found = 0;

  for (let row = 0; row < size; row++) {
    let bitsRow = 0;
    let bitsCol = 0;
    for (let col = 0; col < size; col++) {
      bitsRow = ((bitsRow << 1) & 0x7ff) | grid.get(row, col);
      if (col >= 10 && (bitsRow === 0x5d0 || bitsRow === 0x05d)) {
        found++;
      }
      bitsCol = ((bitsCol << 1) & 0x7ff) | grid.get(col, row);
      if (col >= 10 && (bitsCol === 0x5d0 || bitsCol === 0x05d)) {
        found++;
      }
    }
  }

  return found * 40;
}

/** Deviation of the dark-module proportion from 50%, in 5% steps. */
function penaltyDarkRatio(grid: ModuleGrid): number {
  let darkCount = 0;
  for (let i = 0; i < grid.dark.length; i++) {
    darkCount += grid.dark[i]!;
  }
  const k = Math.abs(Math.ceil((darkCount * 100) / grid.dark.length / 5) - 10);
  return k * 10;
}

/** Tries all eight masks with their format info in place; lowest penalty wins. */
function selectMask(grid: ModuleGrid): number {
  let best = 0;
  let lowest = Infinity;

  for (let pattern = 0; pattern < 8; pattern++) {
    placeFormatInfo(grid, pattern);
    applyMask(grid, pattern);

    const penalty =
      penaltyAdjacent(grid) +
      penaltyBlocks(grid) +
      penaltyFinderLike(grid) +
      penaltyDarkRatio(grid);

    applyMask(grid, pattern); // XOR is its own inverse — undo before the next try

    if (penalty < lowest) {
      lowest = penalty;
      best = pattern;
    }
  }

  return best;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

function utf8Bytes(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

function smallestVersion(byteLength: number): number {
  for (let version = 1; version <= MAX_VERSION; version++) {
    if (byteLength <= byteCapacity(version)) {
      return version;
    }
  }
  throw new Error(
    `QR payload too long: ${byteLength} bytes exceeds the ${byteCapacity(
      MAX_VERSION,
    )}-byte capacity of version ${MAX_VERSION} at error correction level M`,
  );
}

/** Encodes `text` as a QR module matrix (byte mode, level M). */
export function encodeQrMatrix(text: string): QrMatrix {
  const bytes = utf8Bytes(text);
  const version = smallestVersion(bytes.length);
  const grid = new ModuleGrid(symbolSize(version));

  placeFinderPatterns(grid);
  placeTimingPatterns(grid);
  placeAlignmentPatterns(grid, version);
  // Reserve the format area before data placement; the real bits land after
  // the mask is chosen.
  placeFormatInfo(grid, 0);
  placeVersionInfo(grid, version);
  placeData(
    grid,
    interleaveCodewords(buildDataCodewords(bytes, version), version),
  );

  const maskPattern = selectMask(grid);
  placeFormatInfo(grid, maskPattern);
  applyMask(grid, maskPattern);

  const modules: boolean[][] = [];
  for (let row = 0; row < grid.size; row++) {
    const cells: boolean[] = [];
    for (let col = 0; col < grid.size; col++) {
      cells.push(grid.get(row, col) === 1);
    }
    modules.push(cells);
  }

  return { size: grid.size, version, maskPattern, modules };
}

/**
 * Light border around the symbol, in modules. The spec requires at least 4;
 * scanners rely on it to find where the symbol ends.
 */
export const QR_QUIET_ZONE = 4;

/** Side length of the drawing area, in modules, including both quiet zones. */
export function qrSvgExtent(
  matrix: QrMatrix,
  quietZone: number = QR_QUIET_ZONE,
): number {
  return matrix.size + quietZone * 2;
}

/**
 * Builds the SVG path data for the dark modules, offset by the quiet zone.
 *
 * Callers render this into an `<svg>` themselves, which keeps the encoder free of
 * markup concerns and lets consumers use JSX instead of injecting raw HTML.
 *
 * Horizontally adjacent modules merge into one run, so a version-7 symbol emits a
 * few hundred segments instead of ~2000 individual squares.
 */
export function qrMatrixToPath(
  matrix: QrMatrix,
  quietZone: number = QR_QUIET_ZONE,
): string {
  const segments: string[] = [];

  for (let row = 0; row < matrix.size; row++) {
    let runStart = -1;
    for (let col = 0; col <= matrix.size; col++) {
      const dark = col < matrix.size && matrix.modules[row]![col]!;
      if (dark && runStart === -1) {
        runStart = col;
      } else if (!dark && runStart !== -1) {
        const width = col - runStart;
        segments.push(
          `M${runStart + quietZone} ${row + quietZone}h${width}v1h-${width}z`,
        );
        runStart = -1;
      }
    }
  }

  return segments.join("");
}
