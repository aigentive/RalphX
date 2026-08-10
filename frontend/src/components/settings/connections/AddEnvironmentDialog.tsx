// PR 2.5-a — the add-environment wizard.
//
// The UI collects input and renders outcomes. The pairing exchange, the device token,
// and the Keychain write live entirely in Rust (P-18): nothing here ever sees a
// credential, and no step re-derives a protocol comparison the service already made.
//
// There is no retry anywhere in this file (A-5). Every failure renders a state with a
// way back; the USER retries.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, CheckCircle2, Loader2 } from "lucide-react";

import {
  remoteEnvironmentsApi,
  type RemoteEnvironmentPreview,
} from "@/api/remote-environments";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { NoticeBanner } from "@/components/ui/notice-banner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useEnvironmentStore } from "@/stores/environmentStore";

import {
  groupPairingCode,
  normalizePairingHostUrl,
  parseManualPairingCode,
  parsePairingUrl,
  type PairingParseReason,
} from "../remote-access/remote-access-utils";
import {
  classifyPairingError,
  describePairingFailure,
  type PairingFailure,
} from "./pairing-errors";

/**
 * Explicit union rather than a set of booleans: `previewing` and `pairing` are both
 * "busy", but only one of them has consumed a single-use code, and only `blocked` is
 * terminal for the attempt. Collapsing them would make those differences invisible.
 */
type WizardState =
  | { step: "input" }
  | { step: "previewing" }
  | { step: "preview"; preview: RemoteEnvironmentPreview }
  | { step: "pairing"; preview: RemoteEnvironmentPreview }
  | { step: "success"; name: string; environmentRowId: string }
  | { step: "blocked"; failure: PairingFailure }
  | { step: "error"; failure: PairingFailure; from: "input" | "preview" };

const FIELD_ERROR_COPY: Record<PairingParseReason, string> = {
  "not-a-pairing-url": "That is not a RalphX pairing link.",
  "missing-host":
    "Enter the host address shown on the host's Remote Access pane.",
  "missing-code": "Enter the pairing code shown on the host.",
  "code-in-query":
    "This link carries its code in the query string, where it can be logged. Generate a fresh code on the host.",
  "bad-code-prefix": "Pairing codes start with rxp_.",
  "bad-host-url":
    "Use just the host and port, for example studio.tail-x.ts.net:3849.",
  "host-url-has-query": "The host address must not include a query string.",
  "host-url-has-fragment": "The host address must not include a #fragment.",
};

/** Visual grouping only (R-12); the raw code is what pairing receives. */
function renderGroupedCode(code: string): string {
  const { prefix, groups } = groupPairingCode(code);
  return [prefix, ...groups].filter((part) => part.length > 0).join(" ");
}

function shortEnvironmentId(environmentId: string): string {
  return environmentId.length <= 12
    ? environmentId
    : `${environmentId.slice(0, 6)}…${environmentId.slice(-4)}`;
}

export interface AddEnvironmentDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Re-pair: the row's `baseUrl`, locked so a re-pair cannot silently retarget. */
  lockedHost?: string;
  /** Re-pair: the existing row's name. */
  initialName?: string;
  /** Fires after a successful pair, so the pane can re-list from the backend. */
  onPaired?: () => void;
}

export function AddEnvironmentDialog({
  open,
  onOpenChange,
  lockedHost,
  initialName,
  onPaired,
}: AddEnvironmentDialogProps) {
  const [state, setState] = useState<WizardState>({ step: "input" });
  const [hostInput, setHostInput] = useState(lockedHost ?? "");
  const [codeInput, setCodeInput] = useState("");
  const [name, setName] = useState(initialName ?? "");
  const [hostError, setHostError] = useState<string | null>(null);
  const [codeError, setCodeError] = useState<string | null>(null);
  const setActiveEnvironment = useEnvironmentStore(
    (store) => store.setActiveEnvironment,
  );

  // Re-opening the dialog must not resume a previous attempt's step: a consumed code
  // is gone, and a stale `preview` would offer to pair on evidence that has expired.
  const wasOpen = useRef(open);
  useEffect(() => {
    if (open && !wasOpen.current) {
      setState({ step: "input" });
      setHostInput(lockedHost ?? "");
      setCodeInput("");
      setName(initialName ?? "");
      setHostError(null);
      setCodeError(null);
    }
    wasOpen.current = open;
  }, [open, lockedHost, initialName]);

  const parsedHost = useMemo(
    () => normalizePairingHostUrl(hostInput),
    [hostInput],
  );
  const parsedCode = useMemo(
    () => parseManualPairingCode(codeInput),
    [codeInput],
  );
  const canContinue = parsedHost.ok && parsedCode.ok;

  /**
   * A pasted pairing link fills BOTH fields. The code comes from the hash fragment
   * only; a link carrying it in the query is refused rather than accepted, because
   * that code has already travelled somewhere it can be logged.
   */
  const handleHostChange = useCallback((raw: string) => {
    setHostError(null);
    if (!raw.trim().toLowerCase().startsWith("ralphx://pair")) {
      setHostInput(raw);
      return;
    }
    const parsed = parsePairingUrl(raw);
    if (!parsed.ok) {
      setHostInput(raw);
      setHostError(FIELD_ERROR_COPY[parsed.reason]);
      return;
    }
    setHostInput(parsed.host);
    setCodeInput(parsed.code);
    setCodeError(null);
    // The name is NOT prefilled here. It is decided at the verify step, where
    // `alreadyPairedAs` is known — prefilling from the host now would win the
    // "already empty?" check and hide the existing row's name from a re-pair.
  }, []);

  const handleContinue = useCallback(async () => {
    if (!parsedHost.ok) {
      setHostError(FIELD_ERROR_COPY[parsedHost.reason]);
      return;
    }
    if (!parsedCode.ok) {
      setCodeError(FIELD_ERROR_COPY[parsedCode.reason]);
      return;
    }
    setState({ step: "previewing" });
    try {
      const preview = await remoteEnvironmentsApi.preview(parsedHost.url);
      setName((current) =>
        current.trim() !== ""
          ? current
          : (preview.alreadyPairedAs ??
            parsedHost.url.replace(/^https?:\/\//, "")),
      );
      setState({ step: "preview", preview });
    } catch (error) {
      const failure = classifyPairingError(error);
      setState(
        failure.kind === "version"
          ? { step: "blocked", failure }
          : { step: "error", failure, from: "input" },
      );
    }
  }, [parsedHost, parsedCode]);

  const handlePair = useCallback(async () => {
    if (state.step !== "preview" || !parsedHost.ok || !parsedCode.ok) {
      return;
    }
    const trimmedName = name.trim();
    if (trimmedName === "") {
      return;
    }
    setState({ step: "pairing", preview: state.preview });
    try {
      const summary = await remoteEnvironmentsApi.pair(
        parsedHost.url,
        parsedCode.code,
        trimmedName,
      );
      // The pane and the runtime both read the registry from Rust; refreshing here is
      // what makes the new row appear in the switcher.
      await useEnvironmentStore.getState().loadEnvironments();
      onPaired?.();
      setState({
        step: "success",
        name: summary.name,
        environmentRowId: summary.id,
      });
    } catch (error) {
      const failure = classifyPairingError(error);
      setState(
        failure.kind === "version"
          ? { step: "blocked", failure }
          : { step: "error", failure, from: "preview" },
      );
    }
  }, [state, parsedHost, parsedCode, name, onPaired]);

  const busy = state.step === "previewing" || state.step === "pairing";

  const handleOpenChange = useCallback(
    (next: boolean) => {
      // The Rust pairing sequence is staged and reconciler-safe, but there is no way to
      // half-abort it from here, so the dialog stays put while it runs.
      if (!next && state.step === "pairing") {
        return;
      }
      onOpenChange(next);
    },
    [state.step, onOpenChange],
  );

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        className="max-w-md p-5"
        data-testid="add-environment-dialog"
        onEscapeKeyDown={(event) => {
          if (state.step === "pairing") {
            event.preventDefault();
          }
        }}
      >
        <DialogHeader className="mb-4">
          <DialogTitle className="text-sm font-semibold tracking-tight text-[var(--text-primary)]">
            Add environment
          </DialogTitle>
          <DialogDescription className="text-xs text-[var(--text-muted)]">
            {state.step === "input" || state.step === "previewing"
              ? "Paste a pairing link, or enter the host and code shown on the host's Remote Access pane."
              : "Confirm the host identity before pairing."}
          </DialogDescription>
        </DialogHeader>

        {(state.step === "input" || state.step === "previewing") && (
          <div className="space-y-4" data-testid="add-environment-step-connect">
            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-[var(--text-secondary)]"
                htmlFor="add-environment-host"
              >
                Pairing link or host
              </label>
              <Input
                id="add-environment-host"
                data-testid="add-environment-host"
                value={hostInput}
                disabled={busy || lockedHost !== undefined}
                placeholder="ralphx://pair?… or studio.tail-x.ts.net:3849"
                onChange={(event) => handleHostChange(event.target.value)}
                aria-invalid={hostError !== null}
                aria-describedby={
                  hostError ? "add-environment-host-error" : undefined
                }
              />
              {hostError !== null && (
                <p
                  id="add-environment-host-error"
                  data-testid="add-environment-host-error"
                  className="text-xs text-[var(--status-error)]"
                >
                  {hostError}
                </p>
              )}
            </div>

            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-[var(--text-secondary)]"
                htmlFor="add-environment-code"
              >
                Pairing code
              </label>
              <Input
                id="add-environment-code"
                data-testid="add-environment-code"
                value={
                  parsedCode.ok ? renderGroupedCode(parsedCode.code) : codeInput
                }
                disabled={busy}
                placeholder="rxp_ XXXX XXXX XXXX"
                className="font-mono"
                onChange={(event) => {
                  setCodeError(null);
                  setCodeInput(event.target.value);
                }}
                aria-invalid={codeError !== null}
                aria-describedby={
                  codeError ? "add-environment-code-error" : undefined
                }
              />
              {codeError !== null && (
                <p
                  id="add-environment-code-error"
                  data-testid="add-environment-code-error"
                  className="text-xs text-[var(--status-error)]"
                >
                  {codeError}
                </p>
              )}
            </div>

            <div className="flex justify-end gap-2 pt-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                disabled={busy}
                onClick={() => onOpenChange(false)}
              >
                Cancel
              </Button>
              <Button
                type="button"
                size="sm"
                data-testid="add-environment-continue"
                disabled={!canContinue || busy}
                onClick={() => void handleContinue()}
              >
                {state.step === "previewing" ? (
                  <>
                    <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                    Checking…
                  </>
                ) : (
                  "Continue"
                )}
              </Button>
            </div>
          </div>
        )}

        {(state.step === "preview" || state.step === "pairing") && (
          <div className="space-y-4" data-testid="add-environment-step-verify">
            <div
              className="rounded-md p-3"
              style={{
                backgroundColor: "var(--bg-surface, #1e1e23)",
                borderColor: "var(--border-subtle, #2c2c33)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
            >
              <h4 className="mb-2 text-xs font-semibold text-[var(--text-primary)]">
                Host identity
              </h4>
              <dl className="space-y-1 text-xs">
                <div className="flex justify-between gap-3">
                  <dt className="text-[var(--text-muted)]">Environment</dt>
                  <dd
                    className="font-mono text-[var(--text-primary)]"
                    data-testid="add-environment-identity"
                    title={state.preview.environmentId}
                  >
                    {shortEnvironmentId(state.preview.environmentId)}
                  </dd>
                </div>
                <div className="flex justify-between gap-3">
                  <dt className="text-[var(--text-muted)]">RalphX</dt>
                  <dd className="text-[var(--text-primary)]">
                    {state.preview.appVersion}
                  </dd>
                </div>
                <div className="flex justify-between gap-3">
                  <dt className="text-[var(--text-muted)]">Platform</dt>
                  <dd className="text-[var(--text-primary)]">
                    {state.preview.platform}
                  </dd>
                </div>
                <div className="flex justify-between gap-3">
                  <dt className="text-[var(--text-muted)]">Protocol</dt>
                  <dd
                    className="text-[var(--text-primary)]"
                    data-testid="add-environment-protocol"
                  >
                    {/* Reaching this step IS the compatibility verdict — the service
                        already refused a contradiction, so nothing is recomputed here. */}
                    v{state.preview.protocolVersion} · compatible
                  </dd>
                </div>
              </dl>
            </div>

            {state.preview.alreadyPairedAs !== null && (
              <NoticeBanner
                tone="neutral"
                testId="add-environment-already-paired"
              >
                Already paired as “{state.preview.alreadyPairedAs}” — pairing
                again updates it rather than adding a second entry.
              </NoticeBanner>
            )}

            <NoticeBanner tone="neutral" testId="add-environment-agent-control">
              Pairing grants this device agent control on the host. You can revoke it
              later in the host&apos;s Remote access settings.
            </NoticeBanner>

            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-[var(--text-secondary)]"
                htmlFor="add-environment-name"
              >
                Name
              </label>
              <Input
                id="add-environment-name"
                data-testid="add-environment-name"
                value={name}
                disabled={state.step === "pairing"}
                onChange={(event) => setName(event.target.value)}
              />
            </div>

            <div className="flex justify-end gap-2 pt-1">
              {state.step === "pairing" ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span>
                      <Button type="button" variant="ghost" size="sm" disabled>
                        Back
                      </Button>
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>
                    Pairing is in progress and cannot be interrupted
                  </TooltipContent>
                </Tooltip>
              ) : (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  data-testid="add-environment-back"
                  onClick={() => setState({ step: "input" })}
                >
                  Back
                </Button>
              )}
              <Button
                type="button"
                size="sm"
                data-testid="add-environment-pair"
                disabled={state.step === "pairing" || name.trim() === ""}
                onClick={() => void handlePair()}
              >
                {state.step === "pairing" ? (
                  <>
                    <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                    Pairing…
                  </>
                ) : (
                  "Pair"
                )}
              </Button>
            </div>
          </div>
        )}

        {state.step === "blocked" && (
          <div className="space-y-4" data-testid="add-environment-blocked">
            <NoticeBanner
              tone="warning"
              icon={<AlertTriangle className="h-4 w-4" />}
              title={describePairingFailure(state.failure).title}
              testId="add-environment-blocked-banner"
            >
              {describePairingFailure(state.failure).detail}
            </NoticeBanner>
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid="add-environment-blocked-back"
                onClick={() => setState({ step: "input" })}
              >
                Back
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={() => onOpenChange(false)}
              >
                Close
              </Button>
            </div>
          </div>
        )}

        {state.step === "error" && (
          <div className="space-y-4" data-testid="add-environment-error">
            <NoticeBanner
              tone="error"
              icon={<AlertTriangle className="h-4 w-4" />}
              title={describePairingFailure(state.failure).title}
              testId="add-environment-error-banner"
            >
              {describePairingFailure(state.failure).detail}
            </NoticeBanner>
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid="add-environment-error-back"
                onClick={() => setState({ step: "input" })}
              >
                Back
              </Button>
              <Button
                type="button"
                size="sm"
                onClick={() => onOpenChange(false)}
              >
                Close
              </Button>
            </div>
          </div>
        )}

        {state.step === "success" && (
          <div className="space-y-4" data-testid="add-environment-success">
            <NoticeBanner
              tone="success"
              icon={<CheckCircle2 className="h-4 w-4" />}
              title="Paired"
              testId="add-environment-success-banner"
            >
              “{state.name}” is available in the environment switcher.
            </NoticeBanner>
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid="add-environment-switch"
                onClick={() => {
                  void setActiveEnvironment(state.environmentRowId).catch(
                    () => {
                      // Rust refused the switch; the row stays paired and the switcher
                      // still shows it. Closing is honest — nothing was lost.
                    },
                  );
                  onOpenChange(false);
                }}
              >
                Switch to it
              </Button>
              <Button
                type="button"
                size="sm"
                data-testid="add-environment-done"
                onClick={() => onOpenChange(false)}
              >
                Done
              </Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
