// The flow renders the SERVICE's taxonomy. It must never re-derive a version
// comparison or guess at a failure from message prose.

import { describe, expect, it } from "vitest";

import { classifyPairingError, describePairingFailure } from "./pairing-errors";

describe("classifyPairingError", () => {
  it("reads the stable code prefix the IPC boundary emits", () => {
    expect(
      classifyPairingError(
        new Error(
          "REMOTE_VERSION_MISMATCH: host requires client protocol >= 2, this client speaks 1",
        ),
      ),
    ).toEqual({
      kind: "version",
      code: "REMOTE_VERSION_MISMATCH",
      message: "host requires client protocol >= 2, this client speaks 1",
    });
  });

  it("classifies a rejected pairing code as user-fixable", () => {
    expect(classifyPairingError(new Error("PAIRING_REJECTED: code expired"))).toEqual({
      kind: "code",
      code: "PAIRING_REJECTED",
      message: "code expired",
    });
  });

  it("classifies unreachable hosts", () => {
    expect(
      classifyPairingError(new Error("REMOTE_UNREACHABLE: host unreachable: offline")),
    ).toEqual({
      kind: "unreachable",
      code: "REMOTE_UNREACHABLE",
      message: "host unreachable: offline",
    });
  });

  it("classifies a bad URL and a host identity mismatch distinctly", () => {
    expect(classifyPairingError(new Error("INVALID_PAIRING_URL: missing host")).kind).toBe(
      "url",
    );
    expect(
      classifyPairingError(new Error("HOST_IDENTITY_MISMATCH: descriptor a, response b"))
        .kind,
    ).toBe("identity");
  });

  it("falls back to unknown rather than pattern-matching prose", () => {
    // A message with no code prefix is NOT parsed for keywords: guessing "expired"
    // from free text is how a transport failure gets rendered as a bad code.
    const result = classifyPairingError(new Error("the code expired, probably"));
    expect(result.kind).toBe("unknown");
    expect(result.message).toBe("the code expired, probably");
  });

  it("survives non-Error throwables", () => {
    expect(classifyPairingError("boom").kind).toBe("unknown");
    expect(classifyPairingError(undefined).kind).toBe("unknown");
    expect(classifyPairingError(null).message.length).toBeGreaterThan(0);
  });

  it("does not treat an unknown code prefix as a known kind", () => {
    const result = classifyPairingError(new Error("SOME_NEW_CODE: something happened"));
    expect(result.kind).toBe("unknown");
    expect(result.code).toBe("SOME_NEW_CODE");
  });
});

describe("describePairingFailure", () => {
  it("gives every kind an actionable sentence naming what the user does next", () => {
    for (const kind of [
      "code",
      "unreachable",
      "url",
      "identity",
      "unknown",
      "version",
    ] as const) {
      const copy = describePairingFailure({ kind, code: "X", message: "detail" });
      expect(copy.title.length).toBeGreaterThan(0);
      expect(copy.detail.length).toBeGreaterThan(0);
    }
  });

  it("tells the user to generate a fresh code when the code was rejected", () => {
    const copy = describePairingFailure({
      kind: "code",
      code: "PAIRING_REJECTED",
      message: "expired",
    });
    expect(copy.detail).toMatch(/fresh code/i);
  });

  it("carries the service's own message for a version contradiction", () => {
    const copy = describePairingFailure({
      kind: "version",
      code: "REMOTE_VERSION_MISMATCH",
      message: "host requires client protocol >= 2, this client speaks 1",
    });
    expect(copy.detail).toContain("host requires client protocol >= 2");
  });
});
