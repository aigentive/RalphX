# Remote Mobile Transport Spike — Findings

> Status: **PENDING — skeleton only.** No experiment result, verdict, or owner decision is recorded by this document yet.
>
> Scope: PR 0.3 of the Remote Multi-Environment plan. This tracked appendix is the evidence record for R-1 and informs PR 1.1's C-15 CORS layer and the mobile transport specification; it is not a transport implementation or a substitute for the source specification.

## Source contract

- Desktop: the Rust-proxied flow keeps the WKWebView on local `tauri://` IPC; the bearer and remote HTTP/WS connection stay in the client Rust backend (source spec §§2.1, 6.2).
- Direct mobile/browser: the future remote router must use restrictive CORS and handle `OPTIONS` before bearer authentication so preflight cannot receive a 401 (source spec §3.1; C-15).
- Exposure posture to evaluate: Tailscale Serve terminates TLS at the tailnet edge; direct tailnet uses the tailnet address (source spec §4.4 and R-1).

## Preconditions and probe record

| Field | Required record | Status / value |
|---|---|---|
| Host revision | Commit SHA and dirty-tree state used for the probe | Pending |
| Debug harness | Exact debug-only command/listener shape and cfg-gate evidence | Implemented: `debug_start_remote_transport_cors_probe` / `debug_stop_remote_transport_cors_probe` control a fixed `127.0.0.1:0` fixture; both command registration and the module are `#[cfg(debug_assertions)]`-gated. This records harness shape only, not an experiment result. |
| Tailnet access | Logged-in tailnet identity and evidence that Serve is enabled for the host | Pending |
| Serve endpoint | HTTPS/WSS URL used, without pairing codes, bearers, or other secrets | Pending |
| Direct-tailnet endpoint | HTTP/WS URL used, without credentials | Pending |
| Apple probe vehicle | Named macOS `URLSession`/WKWebView harness and, if available, iOS Simulator harness; OS/runtime version | Pending — required before ATS conclusion |
| Browser probe vehicle | Browser/version and origin used for direct-path CORS tests | Pending |
| Evidence storage | Stable tracked artifact links or redacted command output paths | Pending |

## Evidence index

| ID | Question | Capture required | Location | Status |
|---|---|---|---|---|
| E-1 | (a) Desktop Rust-proxy traffic boundary | WKWebView network/devtools capture plus Rust-proxy request log | Pending | Pending |
| E-2 | (b) Auth-before-`OPTIONS` failure | Request/response capture proving the preflight status and CORS headers | Pending | Pending |
| E-3 | (b) Pre-auth-`OPTIONS` success | Request/response capture proving restrictive origin behavior and successful preflight | Pending | Pending |
| E-4 | (c) Serve ATS result | Named Apple probe output for HTTPS/WSS through Serve | Pending | Pending |
| E-5 | (c) Direct-tailnet ATS result | Named Apple probe output for plain tailnet HTTP/WS | Pending | Pending |

## Implemented harness boundary

- The debug-only fixture is isolated in `remote_server::transport_spike`; it binds an ephemeral loopback address only and returns that address to the caller.
- It models only the two direct-browser preflight orderings: fixed 401-before-preflight and pre-auth `OPTIONS` with the fixed development origin `http://127.0.0.1:1420`. It accepts no bearer, pairing code, or remote-listener configuration.
- It is absent from release module compilation and Tauri command registration via `#[cfg(debug_assertions)]`. This is cfg-gate evidence, not release-build execution evidence; the release-build verification remains the final PR 0.3 task.
- No desktop, browser, Serve, direct-tailnet, or ATS experiment has been run or concluded by this harness implementation.

## Desktop proxy-stub code evidence (not a WKWebView capture)

- `debug_run_desktop_proxy_stub` is a debug-only Tauri command: a webview invokes local IPC, while its Rust implementation starts the fixture and makes the fixed `POST /remote/v1/invoke` request itself.
- The stub obtains the request target from its own `127.0.0.1:0` listener bind; command input selects only the two fixture orderings. It accepts no URL, host, bearer, pairing code, or caller-provided path.
- The sibling behavioral test `desktop_proxy_command_uses_the_loopback_fixture_and_reports_its_result` asserts the loopback-only result schema and the Rust-observed fixture response. This is code/test evidence of the intended boundary, not evidence of actual WKWebView network behavior.
- E-1 and question (a)'s verdict remain pending until a native WKWebView/devtools capture is collected; no such capture was performed in this task.

## (a) Does the Rust-proxied desktop transport produce zero WKWebView cross-origin traffic?

### Question

When the desktop remote-shaped flow is exercised, does the WKWebView issue only local `tauri://` IPC while the Rust proxy owns remote HTTP and WebSocket traffic?

### Required setup and evidence

- Record the remote-shaped operation and the local Tauri command it invokes.
- Capture the WKWebView network/devtools view and the Rust-side proxy request/connection log for the same attempt.
- Redact hostnames, device identifiers, pairing codes, and bearer material from retained evidence.

| Field | Record |
|---|---|
| Probe vehicle and version | Pending |
| WKWebView capture | Pending |
| Rust-proxy capture | Pending |
| Cross-origin `fetch` observed from WKWebView | Pending |
| Cross-origin WebSocket observed from WKWebView | Pending |
| WKWebView preflight observed | Pending |
| Evidence IDs | Pending |
| Finding / verdict | Pending |

### Implication slots

- PR 1.1 / C-15 desktop boundary: Pending evidence review.
- Mobile transport specification: Pending; desktop evidence does not answer the direct-client path.

## (b) Direct browser path: what are the CORS and pre-auth `OPTIONS` ordering results?

### Question

Against the debug-only direct-path listener, does auth-before-`OPTIONS` reproduce the required 401-preflight failure, and does pre-auth `OPTIONS` produce the intended restrictive-CORS behavior?

### Required setup and evidence

- Record the exact request origin, method, requested headers, and listener configuration for both orderings.
- Record complete status and relevant `Access-Control-*` headers for the failing and working cases.
- Confirm the origin allowlist is restrictive; do not use :3847's `allow_origin(Any)` behavior as the experiment baseline.

| Field | Auth-before-`OPTIONS` configuration | Pre-auth-`OPTIONS` configuration |
|---|---|---|
| Probe vehicle and version | Pending | Pending |
| Request origin / method / headers | Pending | Pending |
| Listener ordering evidence | Pending | Pending |
| HTTP status | Pending | Pending |
| CORS response headers | Pending | Pending |
| Browser-visible result | Pending | Pending |
| Evidence IDs | Pending | Pending |
| Finding / verdict | Pending | Pending |

### Implication slots

- PR 1.1 / C-15 router middleware ordering and restrictive-origin policy: Pending evidence review.
- Mobile transport specification direct-browser behavior: Pending evidence review.

## (c) ATS: does Serve TLS satisfy Apple-client requirements, and does plain tailnet HTTP need exceptions?

### Question

For each named Apple probe vehicle, does HTTPS/WSS through Tailscale Serve work without an ATS exception, and what happens for plain direct-tailnet HTTP/WS?

### Required setup and evidence

- Use the named Apple probe vehicle from the preconditions table; do not generalize an observation from an unnamed client.
- Record TLS/certificate observations for Serve and the exact ATS diagnostics for every failed request.
- Keep the result matrix separate by probe vehicle and transport rather than inferring an iOS result from macOS.

| Probe vehicle | Serve HTTPS/WSS result | Direct-tailnet HTTP/WS result | ATS exception required | Evidence IDs | Finding |
|---|---|---|---|---|---|
| macOS `URLSession` / WKWebView | Pending | Pending | Pending | Pending | Pending |
| iOS Simulator, if available | Pending | Pending | Pending | Pending | Pending |

### Implication slots

- PR 1.1 endpoint and CORS implementation: Pending evidence review.
- Mobile transport specification ATS policy and any exception requirement: Pending evidence review.

## (d) Verdict: does Serve-only suffice?

### Decision record

| Field | Record |
|---|---|
| Verdict (`yes` / `no` / `insufficient evidence`) | Pending |
| Rationale linked to E-1 through E-5 | Pending |
| Direct-tailnet posture if Serve-only is not selected | Pending |
| Owner decision required | Pending |
| Decision date / owner | Pending |

### Downstream implications

- PR 1.1: Pending — amend or confirm C-15 only after the evidence and owner verdict are recorded.
- Mobile transport specification: Pending — document the selected direct-client transport posture and ATS requirements only after the verdict.

## Open decisions and follow-up

| Decision / follow-up | Owner | Needed before | Status |
|---|---|---|---|
| Choose the debug harness shape: command only or command-controlled throwaway listener | Pending | PR 0.3 harness implementation | Open |
| Name the Apple ATS probe vehicle(s) and record availability | Pending | ATS experiment | Open |
| Provide a Serve-capable logged-in tailnet environment | Pending | Serve experiment | Open |
| Record the Serve-only verdict from captured evidence | Pending | PR 0.3 completion; informational input to PR 1.1 | Open |

## Completion checklist

- [ ] E-1 through E-5 contain redacted, stable evidence links or output.
- [ ] Questions (a) through (d) each have a recorded finding.
- [ ] The Serve-only verdict is explicit and evidence-linked.
- [ ] PR 1.1 C-15 and the mobile transport specification implication slots are filled without contradicting the source contract.
- [ ] The debug harness is confirmed absent from release registration and no :3847/:3848 routing or binding changed.
