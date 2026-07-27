import { describe, expect, it } from "vitest";

import type { AdvertisedEndpoint, RemoteListenerStatus } from "@/api/remote-host";

import {
  buildPairingUrl,
  formatCountdown,
  groupPairingCode,
  pickPreferredEndpoint,
  remainingSeconds,
} from "./remote-access-utils";

const CODE = "rxp_ABCDEFGHJKLMNPQRSTUVWXYZabcdef01";

function status(overrides: Partial<RemoteListenerStatus> = {}): RemoteListenerStatus {
  return {
    enabled: true,
    exposureMode: "serve",
    port: 3849,
    environmentId: "env-1",
    running: true,
    bindAddress: "127.0.0.1:3849",
    serveActive: true,
    serveDegradedReason: null,
    ...overrides,
  };
}

describe("groupPairingCode", () => {
  it("splits the rxp_ prefix from four-character groups (R-12 manual entry)", () => {
    const grouped = groupPairingCode(CODE);
    expect(grouped.prefix).toBe("rxp_");
    expect(grouped.groups).toEqual([
      "ABCD",
      "EFGH",
      "JKLM",
      "NPQR",
      "STUV",
      "WXYZ",
      "abcd",
      "ef01",
    ]);
  });

  it("keeps an unprefixed code intact as grouped chunks", () => {
    const grouped = groupPairingCode("ABCDEFGH");
    expect(grouped.prefix).toBe("");
    expect(grouped.groups).toEqual(["ABCD", "EFGH"]);
  });
});

describe("buildPairingUrl", () => {
  it("puts the code in the hash fragment, never the query (§3.7)", () => {
    const url = buildPairingUrl("https://mac-studio.tailnet.ts.net", CODE);
    expect(url).toBe(
      `ralphx://pair?host=${encodeURIComponent("https://mac-studio.tailnet.ts.net")}#code=${CODE}`,
    );
    const beforeHash = url.split("#")[0] ?? "";
    expect(beforeHash).not.toContain(CODE);
  });
});

describe("pickPreferredEndpoint", () => {
  const serveEndpoint: AdvertisedEndpoint = {
    kind: "loopbackServe",
    url: "https://mac-studio.tailnet.ts.net",
    available: false,
  };
  const directEndpoint: AdvertisedEndpoint = {
    kind: "tailnetDirect",
    url: "http://100.64.0.7:3849",
    available: true,
  };

  it("prefers the first available endpoint", () => {
    expect(pickPreferredEndpoint([serveEndpoint, directEndpoint], status())).toBe(
      "http://100.64.0.7:3849",
    );
  });

  it("falls back to the first endpoint when none is available yet", () => {
    expect(pickPreferredEndpoint([serveEndpoint], status())).toBe(
      "https://mac-studio.tailnet.ts.net",
    );
  });

  it("falls back to the bound address (plain http) for tailnet-direct mode", () => {
    // RALPHX_REMOTE_PORT can override the persisted port, so the fallback must be
    // derived from bindAddress — never from status.port.
    const result = pickPreferredEndpoint(
      null,
      status({
        exposureMode: "tailnetDirect",
        bindAddress: "100.64.0.7:4001",
        port: 3849,
      }),
    );
    expect(result).toBe("http://100.64.0.7:4001");
  });

  it("returns null for serve mode without endpoint data (no fake URL)", () => {
    expect(pickPreferredEndpoint(null, status())).toBeNull();
    expect(pickPreferredEndpoint([], status())).toBeNull();
  });

  it("returns null when tailnet-direct is not running", () => {
    expect(
      pickPreferredEndpoint(
        null,
        status({ exposureMode: "tailnetDirect", running: false, bindAddress: null }),
      ),
    ).toBeNull();
  });
});

describe("remainingSeconds", () => {
  it("counts down toward the expiry timestamp", () => {
    const now = Date.parse("2026-07-27T10:00:00Z");
    expect(remainingSeconds("2026-07-27T10:10:00Z", now)).toBe(600);
    expect(remainingSeconds("2026-07-27T10:00:30Z", now)).toBe(30);
  });

  it("clamps at zero after expiry and on unparseable input", () => {
    const now = Date.parse("2026-07-27T10:00:00Z");
    expect(remainingSeconds("2026-07-27T09:59:00Z", now)).toBe(0);
    expect(remainingSeconds("not-a-date", now)).toBe(0);
  });
});

describe("formatCountdown", () => {
  it("renders M:SS", () => {
    expect(formatCountdown(600)).toBe("10:00");
    expect(formatCountdown(65)).toBe("1:05");
    expect(formatCountdown(0)).toBe("0:00");
  });
});
