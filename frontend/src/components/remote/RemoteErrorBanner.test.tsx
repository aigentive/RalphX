/**
 * `remoteErrorBannerProps` shipped in 2.6 with tests and no call site. These pin the
 * PLACEMENT: the mapper's two codes reach a surface, and everything else still falls
 * through to whatever handling the call site already had.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { RemoteTransportError } from "@/lib/remote/transport-errors";
import { RemoteErrorBanner } from "./RemoteErrorBanner";

function transportError(code: string): RemoteTransportError {
  return new RemoteTransportError({
    code: code as RemoteTransportError["code"],
    message: `${code} happened`,
    environmentId: "env-a",
  });
}

describe("RemoteErrorBanner", () => {
  it("explains a scope refusal instead of asking the user to retry", () => {
    render(<RemoteErrorBanner error={transportError("REMOTE_FORBIDDEN")} />);

    const banner = screen.getByTestId("remote-error-banner");
    expect(banner).toHaveTextContent("Not allowed for this device");
    expect(banner).toHaveAttribute("data-tone", "error");
  });

  it("explains a host-unavailable op", () => {
    render(
      <RemoteErrorBanner error={transportError("REMOTE_COMMAND_UNAVAILABLE")} />
    );

    expect(screen.getByTestId("remote-error-banner")).toHaveTextContent(
      "Unavailable on this host"
    );
  });

  it.each([
    "REMOTE_UNAUTHORIZED",
    "REMOTE_TIMEOUT_UNKNOWN",
    "REMOTE_REQUEST_IN_PROGRESS",
    "REMOTE_UNREACHABLE",
    "REMOTE_VERSION_MISMATCH",
    "REMOTE_INVALID_ARGUMENTS",
    "REMOTE_INTERNAL_ERROR",
  ])("renders nothing for %s, leaving its existing treatment alone", (code) => {
    render(<RemoteErrorBanner error={transportError(code)} />);

    expect(screen.queryByTestId("remote-error-banner")).toBeNull();
  });

  it("renders nothing for a non-transport error or no error at all", () => {
    const { rerender } = render(<RemoteErrorBanner error={new Error("boom")} />);
    expect(screen.queryByTestId("remote-error-banner")).toBeNull();

    rerender(<RemoteErrorBanner error={null} />);
    expect(screen.queryByTestId("remote-error-banner")).toBeNull();
  });
});
