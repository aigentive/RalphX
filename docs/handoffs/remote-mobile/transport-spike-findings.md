# Remote Mobile Transport Spike — Findings

> Status: **PARTIAL — E-2/E-3 socket evidence captured; E-1/E-4/E-5 remain missing or blocked.** The only overall verdict recorded below is insufficient evidence; no transport or owner decision is implied.
>
> Scope: PR 0.3 of the Remote Multi-Environment plan. This tracked appendix is the evidence record for R-1 and informs PR 1.1's C-15 CORS layer and the mobile transport specification; it is not a transport implementation or a substitute for the source specification.

## Source contract

- Desktop: the Rust-proxied flow keeps the WKWebView on local `tauri://` IPC; the bearer and remote HTTP/WS connection stay in the client Rust backend (source spec §§2.1, 6.2).
- Direct mobile/browser: the future remote router must use restrictive CORS and handle `OPTIONS` before bearer authentication so preflight cannot receive a 401 (source spec §3.1; C-15).
- Exposure posture to evaluate: Tailscale Serve terminates TLS at the tailnet edge; direct tailnet uses the tailnet address (source spec §4.4 and R-1).

## Preconditions and probe record

| Field | Required record | Status / value |
|---|---|---|
| Host revision | Commit SHA and dirty-tree state used for the probe | Pre-task-6 HEAD `eb67a67451dd0cb5792095a960743839fbfd10cd`; worktree clean. |
| Debug harness | Exact debug-only command/listener shape and cfg-gate evidence | Resolved: `debug_start_remote_transport_cors_probe` / `debug_stop_remote_transport_cors_probe` control the implemented command-controlled ephemeral `127.0.0.1:0` loopback listener; both command registration and the module are `#[cfg(debug_assertions)]`-gated. This resolves harness shape only, not a transport result. |
| Tailnet access | Logged-in tailnet identity and evidence that Serve is enabled for the host | Blocked — 2026-07-27 read-only audit found no `tailscale` executable in `PATH`; no logged-in Serve-capable tailnet evidence is available. |
| Serve endpoint | HTTPS/WSS URL used, without pairing codes, bearers, or other secrets | Blocked — no Serve-capable tailnet or endpoint is available; no request was sent. |
| Direct-tailnet endpoint | HTTP/WS URL used, without credentials | Blocked — no tailnet endpoint is available; no request was sent. |
| Apple probe vehicle | Named macOS `URLSession`/WKWebView harness and, if available, iOS Simulator harness; OS/runtime version | macOS 15.7.4 (24G517), Xcode 26.3 (17C529), and an available iOS 26.3 `iPhone 17 Pro` simulator were observed on 2026-07-27. The simulator remained shut down and no URLSession, WKWebView, or simulator probe was run. |
| Browser probe vehicle | Browser/version and origin used for direct-path CORS tests | Pending |
| Evidence storage | Stable tracked artifact links or redacted command output paths | Pending |

## Evidence index

| ID | Question | Capture required | Location | Status |
|---|---|---|---|---|
| E-1 | (a) Desktop Rust-proxy traffic boundary | WKWebView network/devtools capture plus Rust-proxy request log | Pending | Pending |
| E-2 | (b) Auth-before-`OPTIONS` failure | Actual loopback socket request/response | `actual_listener_auth_before_options_returns_401_without_cors_headers` | Captured — Rust socket evidence, not a browser capture |
| E-3 | (b) Pre-auth-`OPTIONS` success and restrictive rejection | Actual loopback socket request/response | `actual_listener_options_before_auth_returns_restrictive_cors_for_allowed_origin`; `actual_listener_options_before_auth_denies_an_unlisted_origin_without_cors_headers` | Captured — Rust socket evidence, not a browser capture |
| E-4 | (c) Serve ATS result | Named Apple probe output for HTTPS/WSS through Serve | Blocked — no logged-in Serve-capable tailnet or endpoint | Not executed; insufficient evidence |
| E-5 | (c) Direct-tailnet ATS result | Named Apple probe output for plain tailnet HTTP/WS | Blocked — no direct-tailnet endpoint | Not executed; insufficient evidence |

## Implemented harness boundary

- The debug-only fixture is isolated in `remote_server::transport_spike`; it binds an ephemeral loopback address only and returns that address to the caller.
- It models only the two direct-browser preflight orderings: fixed 401-before-preflight and pre-auth `OPTIONS` with the fixed development origin `http://127.0.0.1:1420`. It accepts no bearer, pairing code, or remote-listener configuration.
- It is absent from release module compilation and Tauri command registration via `#[cfg(debug_assertions)]`. On 2026-07-27, `cargo check --manifest-path src-tauri/Cargo.toml --release --lib` completed successfully from the content now at rebased pre-task-7 HEAD `011dccef0`; this compiles the library/command registry with `debug_assertions` off. No Rust tests ran in that command.
- Apart from the actual-listener Rust socket tests recorded as E-2/E-3 below, no desktop, browser, Serve, direct-tailnet, or ATS experiment has been run or concluded by this harness implementation.

## Desktop proxy-stub code evidence (not a WKWebView capture)

- `debug_run_desktop_proxy_stub` is a debug-only Tauri command: a webview invokes local IPC, while its Rust implementation starts the fixture and makes the fixed `POST /remote/v1/invoke` request itself.
- The stub obtains the request target from its own `127.0.0.1:0` listener bind; command input selects only the two fixture orderings. It accepts no URL, host, bearer, pairing code, or caller-provided path.
- The sibling behavioral test `desktop_proxy_command_uses_the_loopback_fixture_and_reports_its_result` asserts the loopback-only result schema and the Rust-observed fixture response. This is code/test evidence of the intended boundary, not evidence of actual WKWebView network behavior.
- E-1 and question (a)'s verdict remain pending until a native WKWebView/devtools capture is collected; no such capture was performed in this task.

## E-2 / E-3 actual listener socket evidence (not a browser capture)

- Probe vehicle: the focused Rust sibling tests named in the evidence index use `tokio::net::TcpStream` against the command's actual ephemeral `127.0.0.1:0` listener. They do not call `Router::oneshot` for this evidence.
- Request shape: `OPTIONS /remote/v1/invoke` with `Origin`, `Access-Control-Request-Method: POST`, and `Access-Control-Request-Headers: authorization,content-type`. No bearer, pairing code, or remote host is supplied.
- E-2: with `AuthBeforeOptions`, the actual listener returns `401 Unauthorized` and emits no `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, or `Access-Control-Allow-Headers` header.
- E-3 allowed origin: with `OptionsBeforeAuth` and `Origin: http://127.0.0.1:1420`, the actual listener returns `204 No Content` with `Access-Control-Allow-Origin` set only to that origin, methods `POST`, headers `authorization,content-type`, and `Vary: Origin`.
- E-3 unlisted origin: with `OptionsBeforeAuth` and `Origin: https://unlisted.example`, the actual listener returns `403 Forbidden` and emits no CORS allow headers.
- These are listener/socket assertions recorded by the focused Rust test suite. No browser page, browser network inspector, or WKWebView capture was run, so browser-visible results remain pending.

## E-4 / E-5 ATS prerequisite audit — blocked (no transport result)

- Audit date: 2026-07-27. This was read-only environment inspection; it did not install software, start the simulator, configure Serve, or send a network request.
- The `tailscale` command was unavailable in `PATH`. Therefore this worktree has no verified logged-in tailnet identity, Serve capability, Serve HTTPS/WSS endpoint, or direct-tailnet HTTP/WS endpoint for the required probes.
- The host was macOS 15.7.4 (build 24G517) with Xcode 26.3 (build 17C529). An iOS 26.3 `iPhone 17 Pro` simulator is available for a future named vehicle, but it was shutdown and was not started.
- A future actual run must use a named macOS `URLSession`/WKWebView probe and, if selected, that iOS Simulator vehicle, after a logged-in Serve-capable tailnet supplies redacted Serve and direct-tailnet endpoints.
- E-4 and E-5 are **not executed** and the ATS/direct-tailnet outcomes are **not inferred**. This task is blocked with insufficient evidence rather than a Serve-only verdict.

## Release configuration and routing-scope evidence

- Release cfg proof: `cargo check --manifest-path src-tauri/Cargo.toml --release --lib` completed successfully on 2026-07-27 from the content now at rebased pre-task-7 HEAD `011dccef0`. It compiles the command registry/library with `debug_assertions` off; because the transport-spike module and each registration are `#[cfg(debug_assertions)]`, the release configuration contains no transport-spike code path. A separate source-text guard was not added because this release compilation is the stronger deterministic check.
- Routing/binding diff proof: `git diff --name-only 35e8242f7..011dccef0 -- src-tauri/src/http_server src-tauri/src/utils/backend_endpoint.rs` returned no paths. `35e8242f7` is the rebased pre-0.3 base and `011dccef0` the rebased pre-task-7 Phase-0.3 HEAD.
- Therefore this PR 0.3 diff does not change `src-tauri/src/http_server/**`, `backend_endpoint.rs`, or the production :3847/:3848 routing/binding configuration. The debug fixture remains separate on ephemeral loopback only.
- The release check started no Rust tests. `cd src-tauri && cargo clean` ran afterward for disk hygiene and removed the generated check artifacts.

## (a) Does the Rust-proxied desktop transport produce zero WKWebView cross-origin traffic?

### Question

When the desktop remote-shaped flow is exercised, does the WKWebView issue only local `tauri://` IPC while the Rust proxy owns remote HTTP and WebSocket traffic?

### Required setup and evidence

- Record the remote-shaped operation and the local Tauri command it invokes.
- Capture the WKWebView network/devtools view and the Rust-side proxy request/connection log for the same attempt.
- Redact hostnames, device identifiers, pairing codes, and bearer material from retained evidence.

| Field | Record |
|---|---|
| Probe vehicle and version | No native WKWebView probe vehicle was run. |
| WKWebView capture | Missing — no devtools/network capture was collected. |
| Rust-proxy capture | The debug command test records the Rust-observed fixed loopback response only; it is not a proxy connection log for a native attempt. |
| Cross-origin `fetch` observed from WKWebView | Unobserved — no native capture. |
| Cross-origin WebSocket observed from WKWebView | Unobserved — no native capture. |
| WKWebView preflight observed | Unobserved — no native capture. |
| Evidence IDs | Code/test evidence only: `desktop_proxy_command_uses_the_loopback_fixture_and_reports_its_result`; E-1 capture missing. |
| Finding / verdict | The code/test boundary exists, but the required native proof is missing; desktop claim remains **insufficient evidence**. |

### Implication slots

- PR 1.1 / C-15 desktop boundary: the code/test seam is consistent with a Rust-side proxy boundary, but the required native WKWebView capture is missing; do not treat the desktop claim as confirmed.
- Mobile transport specification / R-1: desktop code evidence does not answer the direct-client path; the residual remains open.

**Recorded finding:** (a) is not confirmed. The debug Rust-side loopback test is useful boundary evidence, but it cannot establish which requests left a WKWebView.

## (b) Direct browser path: what are the CORS and pre-auth `OPTIONS` ordering results?

### Question

Against the debug-only direct-path listener, does auth-before-`OPTIONS` reproduce the required 401-preflight failure, and does pre-auth `OPTIONS` produce the intended restrictive-CORS behavior?

### Required setup and evidence

- Record the exact request origin, method, requested headers, and listener configuration for both orderings.
- Record complete status and relevant `Access-Control-*` headers for the failing and working cases.
- Confirm the origin allowlist is restrictive; do not use :3847's `allow_origin(Any)` behavior as the experiment baseline.

| Field | Auth-before-`OPTIONS` configuration | Pre-auth-`OPTIONS` configuration |
|---|---|---|
| Probe vehicle and version | Focused Rust `tokio::net::TcpStream` sibling test (not a browser) | Focused Rust `tokio::net::TcpStream` sibling tests (not a browser) |
| Request origin / method / headers | `OPTIONS /remote/v1/invoke`; `Origin: http://127.0.0.1:1420`; requested `POST`, `authorization,content-type` | Allowed case: same fixed origin/request; rejected case: `Origin: https://unlisted.example`; same requested method/headers |
| Listener ordering evidence | `DebugCorsProbeOrdering::AuthBeforeOptions` on actual ephemeral loopback listener | `DebugCorsProbeOrdering::OptionsBeforeAuth` on actual ephemeral loopback listener |
| HTTP status | `401 Unauthorized` | Allowed: `204 No Content`; unlisted: `403 Forbidden` |
| CORS response headers | No allow-origin/methods/headers | Allowed: fixed allow-origin, `POST`, `authorization,content-type`, `Vary: Origin`; unlisted: no allow headers |
| Browser-visible result | Pending | Pending |
| Evidence IDs | E-2 — actual socket test named above | E-3 — actual socket tests named above |
| Finding / verdict | The modeled auth-before-OPTIONS ordering reproduces the required preflight-401 failure. Browser result remains pending. | The modeled pre-auth OPTIONS ordering has restrictive success for only the fixed origin and rejects an unlisted origin. Browser result remains pending. |

### Implication slots

- PR 1.1 / C-15 router middleware ordering and restrictive-origin policy: the actual-listener socket evidence supports pre-auth `OPTIONS` and a fixed allowlist; browser evidence is still pending.
- Mobile transport specification direct-browser behavior: Pending browser evidence review; the socket fixture is not a mobile/browser observation.

**Recorded finding:** (b) confirms the debug listener's order-dependent socket behavior and fixed-origin policy, not browser-visible CORS behavior.

## (c) ATS: does Serve TLS satisfy Apple-client requirements, and does plain tailnet HTTP need exceptions?

### Question

For each named Apple probe vehicle, does HTTPS/WSS through Tailscale Serve work without an ATS exception, and what happens for plain direct-tailnet HTTP/WS?

### Required setup and evidence

- Use the named Apple probe vehicle from the preconditions table; do not generalize an observation from an unnamed client.
- Record TLS/certificate observations for Serve and the exact ATS diagnostics for every failed request.
- Keep the result matrix separate by probe vehicle and transport rather than inferring an iOS result from macOS.

| Probe vehicle | Serve HTTPS/WSS result | Direct-tailnet HTTP/WS result | ATS exception required | Evidence IDs | Finding |
|---|---|---|---|---|---|
| macOS `URLSession` / WKWebView | Not executed — no Serve endpoint | Not executed — no direct-tailnet endpoint | Insufficient evidence | E-4/E-5 blocked prerequisite audit | Preserve as a future named probe vehicle |
| iOS 26.3 `iPhone 17 Pro` Simulator (available, shutdown) | Not executed — no Serve endpoint | Not executed — no direct-tailnet endpoint | Insufficient evidence | E-4/E-5 blocked prerequisite audit | Preserve as a future named probe vehicle; simulator was not started |

### Implication slots

- PR 1.1 / C-15: no Serve/ATS result confirms or amends the endpoint posture. The only confirmed router implication remains pre-auth `OPTIONS` plus a fixed restrictive allowlist from E-2/E-3.
- Mobile transport specification / R-1: ATS policy, direct-tailnet posture, and any exception requirement remain unselected because E-4/E-5 were not executed.

**Recorded finding:** (c) is blocked before a transport request; neither Serve TLS nor plain direct-tailnet ATS behavior was observed.

## (d) Verdict: does Serve-only suffice?

### Decision record

| Field | Record |
|---|---|
| Verdict (`yes` / `no` / `insufficient evidence`) | **Insufficient evidence** |
| Rationale linked to E-1 through E-5 | E-1 lacks native WKWebView/devtools capture; E-2/E-3 establish only actual loopback socket ordering behavior; E-4/E-5 were blocked before any Apple/Serve/direct-tailnet request. |
| Direct-tailnet posture if Serve-only is not selected | Unselected — no plain-tailnet behavior or ATS result was observed. |
| Owner decision required | Yes — choose Serve-only or a direct-tailnet posture only after the missing evidence exists. |
| Decision date / owner | Pending owner decision; no date recorded. |

### Downstream implications

- PR 1.1 / C-15: carry forward the confirmed E-2/E-3 rule: handle `OPTIONS` before bearer authentication and permit only the fixed allowlisted origin. Do not claim the desktop WKWebView boundary or Serve posture is confirmed.
- Mobile transport specification / R-1: retain the direct-client CORS/ATS and Serve-vs-direct-tailnet questions as residual gaps; do not select a transport or ATS exception policy from this appendix.

## Open decisions and follow-up

| Decision / follow-up | Owner | Needed before | Status |
|---|---|---|---|
| Choose the debug harness shape: command only or command-controlled throwaway listener | Resolved in the implemented debug-only command-controlled ephemeral loopback listener | PR 0.3 harness implementation | Resolved |
| Name the Apple ATS probe vehicle(s) and record availability | Future macOS `URLSession`/WKWebView vehicle; iOS 26.3 `iPhone 17 Pro` Simulator observed available and shutdown on 2026-07-27 | ATS experiment | Open — named vehicles were not run |
| Provide a Serve-capable logged-in tailnet environment | Pending | Serve experiment | Blocked — `tailscale` unavailable in `PATH`; no endpoint to probe |
| Record the Serve-only verdict from captured evidence | Pending | PR 0.3 completion; informational input to PR 1.1 | Blocked — E-4/E-5 not executed, insufficient evidence |

## Completion checklist

- [ ] E-1 through E-5 contain redacted, stable evidence links or output.
- [ ] Questions (a) through (d) each have a recorded finding.
- [ ] The Serve-only verdict is explicit and evidence-linked.
- [ ] PR 1.1 C-15 and the mobile transport specification implication slots are filled without contradicting the source contract.
- [x] The debug harness is confirmed absent from release registration and no :3847/:3848 routing or binding changed. *(Release library check and scoped pre-0.3 diff recorded above; this does not close E-1/E-4/E-5.)*
