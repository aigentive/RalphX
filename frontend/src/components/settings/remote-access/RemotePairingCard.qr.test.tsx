import { render, screen, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { MintedRemotePairingCode } from "@/api/remote-host";
import { TooltipProvider } from "@/components/ui/tooltip";
import { encodeQrMatrix, qrMatrixToPath } from "@/lib/qr/qr-encode";

import { RemotePairingCard } from "./RemotePairingCard";
import { buildPairingUrl } from "./remote-access-utils";

const ENDPOINT = "http://192.168.1.50:8443";

/** The rendered symbol, read straight off the DOM rather than off a string. */
function renderedQrPath(): string {
  return (
    screen
      .getByTestId("remote-pairing-qr")
      .querySelector("path")
      ?.getAttribute("d") ?? ""
  );
}

/** The symbol the pane is supposed to be showing for this endpoint and code. */
function expectedQrPath(host: string, code: string): string {
  return qrMatrixToPath(encodeQrMatrix(buildPairingUrl(host, code)));
}

function minted(
  overrides: Partial<MintedRemotePairingCode> = {},
): MintedRemotePairingCode {
  return {
    id: "code-1",
    code: "rxp_ABCD1234EFGH5678",
    scopes: [],
    createdAt: "2026-07-27T10:00:00Z",
    expiresAt: "2026-07-27T10:10:00Z",
    expiresInSecs: 600,
    ...overrides,
  };
}

function renderCard(
  props: Partial<React.ComponentProps<typeof RemotePairingCard>> = {},
) {
  const merged = {
    pairing: minted(),
    pairingBusy: false,
    listenerEnabled: true,
    preferredEndpoint: ENDPOINT,
    outstandingCodes: [],
    onGenerate: vi.fn(),
    onCancel: vi.fn(),
    onExpired: vi.fn(),
    ...props,
  };
  return render(
    <TooltipProvider>
      <RemotePairingCard {...merged} />
    </TooltipProvider>,
  );
}

/** Flushes the after-paint frame the QR encode is scheduled on. */
async function settlePaint() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(50);
  });
}

beforeEach(() => {
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "requestAnimationFrame",
      "cancelAnimationFrame",
      "Date",
    ],
  });
  vi.setSystemTime(new Date("2026-07-27T10:00:00Z"));
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("RemotePairingCard — pairing QR", () => {
  it("renders a QR once a code is active", async () => {
    renderCard();
    await settlePaint();

    const qr = screen.getByTestId("remote-pairing-qr");
    expect(qr.querySelector("svg")).not.toBeNull();
  });

  it("encodes exactly the preferred-endpoint pairing URL (R-12: one endpoint per code)", async () => {
    const pairing = minted();
    renderCard({ pairing });
    await settlePaint();

    expect(renderedQrPath()).toBe(expectedQrPath(ENDPOINT, pairing.code));
    expect(renderedQrPath()).not.toBe("");
  });

  it("renders no QR when there is no pairing code", async () => {
    renderCard({ pairing: null });
    await settlePaint();

    expect(screen.queryByTestId("remote-pairing-qr")).not.toBeInTheDocument();
  });

  it("renders no QR when no endpoint is advertised — there is no URL to encode", async () => {
    renderCard({ preferredEndpoint: null });
    await settlePaint();

    expect(screen.queryByTestId("remote-pairing-qr")).not.toBeInTheDocument();
    // The manual path is unaffected and still primary.
    expect(screen.getByTestId("remote-pairing-code")).toBeInTheDocument();
    expect(
      screen.getByTestId("remote-pairing-url-unavailable"),
    ).toBeInTheDocument();
  });

  it("renders no QR once the code has expired", async () => {
    renderCard();
    await settlePaint();
    expect(screen.getByTestId("remote-pairing-qr")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(11 * 60 * 1000);
    });

    expect(screen.queryByTestId("remote-pairing-qr")).not.toBeInTheDocument();
  });

  it("re-encodes when the code is regenerated", async () => {
    const { rerender } = renderCard();
    await settlePaint();
    const first = renderedQrPath();

    const regenerated = minted({ id: "code-2", code: "rxp_ZZZZ9999YYYY8888" });
    rerender(
      <TooltipProvider>
        <RemotePairingCard
          pairing={regenerated}
          pairingBusy={false}
          listenerEnabled
          preferredEndpoint={ENDPOINT}
          outstandingCodes={[]}
          onGenerate={vi.fn()}
          onCancel={vi.fn()}
          onExpired={vi.fn()}
        />
      </TooltipProvider>,
    );
    await settlePaint();

    const second = renderedQrPath();
    expect(second).not.toBe(first);
    expect(second).toBe(expectedQrPath(ENDPOINT, regenerated.code));
  });

  it("re-encodes when the preferred endpoint changes", async () => {
    const { rerender } = renderCard();
    await settlePaint();
    const first = renderedQrPath();

    rerender(
      <TooltipProvider>
        <RemotePairingCard
          pairing={minted()}
          pairingBusy={false}
          listenerEnabled
          preferredEndpoint="http://10.0.0.9:8443"
          outstandingCodes={[]}
          onGenerate={vi.fn()}
          onCancel={vi.fn()}
          onExpired={vi.fn()}
        />
      </TooltipProvider>,
    );
    await settlePaint();

    expect(renderedQrPath()).not.toBe(first);
    expect(renderedQrPath()).toBe(
      expectedQrPath("http://10.0.0.9:8443", minted().code),
    );
  });
});

describe("RemotePairingCard — QR does not block first paint (rule 24)", () => {
  it("paints the card and the manual code before any QR encoding runs", () => {
    renderCard();

    // Synchronous commit: the manual path — which is the primary one — is
    // already on screen, and the QR placeholder holds its space.
    expect(screen.getByTestId("remote-pairing-code")).toBeInTheDocument();
    expect(screen.getByTestId("remote-pairing-qr")).toBeInTheDocument();

    // ...but the matrix has NOT been computed yet; no SVG exists in this commit.
    expect(
      screen.getByTestId("remote-pairing-qr").querySelector("svg"),
    ).toBeNull();
  });

  it("produces the QR only after the paint boundary", async () => {
    renderCard();
    expect(
      screen.getByTestId("remote-pairing-qr").querySelector("svg"),
    ).toBeNull();

    await settlePaint();

    expect(
      screen.getByTestId("remote-pairing-qr").querySelector("svg"),
    ).not.toBeNull();
  });

  it("does not encode at all if the card unmounts before the frame fires", async () => {
    const { unmount } = renderCard();
    unmount();

    // The scheduled job must be cancelled; flushing timers may not throw or warn.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });

    expect(screen.queryByTestId("remote-pairing-qr")).not.toBeInTheDocument();
  });
});

describe("RemotePairingCard — QR renders scannably", () => {
  it("uses literal black-on-white, never theme tokens (WKWebView + scanability)", async () => {
    renderCard();
    await settlePaint();

    const svg = screen.getByTestId("remote-pairing-qr").querySelector("svg");
    const markup = svg?.outerHTML ?? "";
    expect(markup).toContain("#000000");
    expect(markup).toContain("#FFFFFF");
    expect(markup).not.toContain("var(--");
  });

  it("sits on an explicit white surface so a dark theme cannot invert it", async () => {
    renderCard();
    await settlePaint();

    const qr = screen.getByTestId("remote-pairing-qr");
    expect(qr).toHaveStyle({ backgroundColor: "#FFFFFF" });
  });

  it("labels the QR for assistive technology", async () => {
    renderCard();
    await settlePaint();

    expect(screen.getByTestId("remote-pairing-qr")).toHaveAccessibleName(
      /scan/i,
    );
  });
});
