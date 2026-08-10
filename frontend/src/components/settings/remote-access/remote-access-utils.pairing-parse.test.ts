// PR 2.5-a: the client half of R-12 — parsing what the host pane builds.

import { describe, expect, it } from "vitest";

import {
  buildPairingUrl,
  normalizePairingHostUrl,
  parsePairingUrl,
  parseManualPairingCode,
} from "./remote-access-utils";

describe("parsePairingUrl", () => {
  it("round-trips the host pane's own buildPairingUrl output", () => {
    const url = buildPairingUrl("https://studio.tail-x.ts.net:3849", "rxp_ABCD1234EFGH");
    expect(parsePairingUrl(url)).toEqual({
      ok: true,
      host: "https://studio.tail-x.ts.net:3849",
      code: "rxp_ABCD1234EFGH",
    });
  });

  it("reads the code from the hash fragment ONLY (§3.7)", () => {
    // A code in the query string travelled through intermediaries; it is burned.
    const result = parsePairingUrl(
      "ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849&code=rxp_ABCD1234",
    );
    expect(result).toEqual({ ok: false, reason: "code-in-query" });
  });

  it("rejects a query-borne code even when a fragment code is also present", () => {
    const result = parsePairingUrl(
      "ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849&code=rxp_LEAKED#code=rxp_FRESH",
    );
    expect(result).toEqual({ ok: false, reason: "code-in-query" });
  });

  it("rejects a non-pairing URL", () => {
    expect(parsePairingUrl("https://example.com/#code=rxp_ABCD")).toEqual({
      ok: false,
      reason: "not-a-pairing-url",
    });
  });

  it("rejects a pairing URL with no host param", () => {
    expect(parsePairingUrl("ralphx://pair?x=1#code=rxp_ABCD")).toEqual({
      ok: false,
      reason: "missing-host",
    });
  });

  it("rejects a pairing URL with no fragment code", () => {
    expect(parsePairingUrl("ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849")).toEqual({
      ok: false,
      reason: "missing-code",
    });
  });

  it("rejects a fragment code without the rxp_ prefix", () => {
    expect(
      parsePairingUrl("ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849#code=ABCD1234"),
    ).toEqual({ ok: false, reason: "bad-code-prefix" });
  });

  it("rejects a host param carrying its own query or fragment", () => {
    expect(
      parsePairingUrl(
        "ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849%3Fx%3D1#code=rxp_ABCD",
      ),
    ).toEqual({ ok: false, reason: "host-url-has-query" });
    expect(
      parsePairingUrl(
        "ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849%23frag#code=rxp_ABCD",
      ),
    ).toEqual({ ok: false, reason: "host-url-has-fragment" });
  });

  it("never throws on garbage", () => {
    for (const raw of ["", "   ", "not a url", "ralphx://", "://"]) {
      expect(() => parsePairingUrl(raw)).not.toThrow();
      expect(parsePairingUrl(raw).ok).toBe(false);
    }
  });
});

describe("normalizePairingHostUrl", () => {
  it("accepts bare host:port and defaults to https", () => {
    expect(normalizePairingHostUrl("studio.tail-x.ts.net:3849")).toEqual({
      ok: true,
      url: "https://studio.tail-x.ts.net:3849",
    });
  });

  it("preserves an explicit http scheme (tailnet-direct plaintext inside WireGuard)", () => {
    expect(normalizePairingHostUrl("http://100.101.102.103:3849")).toEqual({
      ok: true,
      url: "http://100.101.102.103:3849",
    });
  });

  it("strips a trailing slash so base_url derivation stays canonical", () => {
    expect(normalizePairingHostUrl("https://h.ts.net:3849/")).toEqual({
      ok: true,
      url: "https://h.ts.net:3849",
    });
  });

  it("rejects a query string — nothing unshaped may reach join/ws URL derivation", () => {
    expect(normalizePairingHostUrl("https://h.ts.net:3849?x=1")).toEqual({
      ok: false,
      reason: "host-url-has-query",
    });
  });

  it("rejects a fragment", () => {
    expect(normalizePairingHostUrl("https://h.ts.net:3849#frag")).toEqual({
      ok: false,
      reason: "host-url-has-fragment",
    });
  });

  it("rejects unsupported schemes and empty hosts", () => {
    expect(normalizePairingHostUrl("ftp://h.ts.net")).toEqual({
      ok: false,
      reason: "bad-host-url",
    });
    expect(normalizePairingHostUrl("  ")).toEqual({ ok: false, reason: "missing-host" });
  });
});

describe("parseManualPairingCode", () => {
  it("accepts the grouped form the host pane displays", () => {
    expect(parseManualPairingCode("rxp_ ABCD 1234 EFGH")).toEqual({
      ok: true,
      code: "rxp_ABCD1234EFGH",
    });
  });

  it("accepts the ungrouped canonical form", () => {
    expect(parseManualPairingCode("rxp_ABCD1234EFGH")).toEqual({
      ok: true,
      code: "rxp_ABCD1234EFGH",
    });
  });

  it("strips interior whitespace of any width", () => {
    expect(parseManualPairingCode("  rxp_\tABCD\n1234  ")).toEqual({
      ok: true,
      code: "rxp_ABCD1234",
    });
  });

  it("rejects a missing prefix", () => {
    expect(parseManualPairingCode("ABCD1234")).toEqual({
      ok: false,
      reason: "bad-code-prefix",
    });
  });

  it("rejects a prefix with no body", () => {
    expect(parseManualPairingCode("rxp_")).toEqual({ ok: false, reason: "missing-code" });
  });
});
