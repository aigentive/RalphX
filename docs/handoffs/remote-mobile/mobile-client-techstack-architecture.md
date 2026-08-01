# RalphX Mobile Client — Tech Stack & Architecture Summary (2026-08-01)

A mobile app that syncs to a RalphX host exactly the way the desktop client does. Grounded in the shipped remote protocol (`.artifacts/specs/remote-multi-env/source-spec.md`, §2–§4, §3.7 mobile non-preclusion checklist) and the desktop client's transport architecture. Companion: `full-remote-management-implementation-spec.md` (everything registered on the facade is automatically available to mobile — the protocol is client-agnostic).

## 1. What "sync" means here (identical to desktop)

The client does not replicate a database. It is a **snapshot-hydrate + live-event-stream** peer:

1. **Invoke plane** — `POST /remote/v1/invoke {requestId, cmd, args}` with a bearer device token; same JSON results the local UI gets from Tauri IPC. All reads (projects, kanban, transcripts, artifacts, reviews) are plain invokes.
2. **Event plane** — `GET /remote/v1/events?ticket=…` WebSocket. Durable host-side sequencer assigns cursors; the client tracks `(streamEpoch, cursor)` and resumes from its cursor on reconnect **within the same epoch**. The host mints a fresh epoch every boot → epoch mismatch means "your cursor is worthless, cold-hydrate" (re-run the snapshot invokes, then stream).
3. **Fetch plane** — the small allowlisted set of remounted `/api/*` GET routes (review context, plans, workflows) plus `/remote/v1/attachments/:id` for binaries.
4. **Offline** — read-only cached views + a reconnect banner. No offline mutation outbox in v1 (spec non-goal); mutations require liveness.
5. **Mutations with side-effect latency** — the intent-row pattern: persist an intent (`request_remote_agent_conversation_start/_message/_stop`), host dispatcher executes, client polls the paired `get_*_request` read to a terminal status. Mobile reuses this untouched — it is just invokes.

State authority is always the host. The client renders projections and never trusts its cache to authorize an affordance (scopes gate UI; `presentation === connected` gates writes).

## 2. Connection & auth (identical contract, different plumbing)

| Concern | Desktop client | Mobile client |
|---|---|---|
| Reachability | Tailscale Serve (TLS) or tailnet-direct | Same — Tailscale iOS/Android app puts the phone on the tailnet; the RalphX app just dials the URL |
| Who holds the bearer | Client's Rust proxy (webview never sees it) | OS secure enclave: iOS Keychain / Android Keystore, accessed only by a thin native storage module |
| HTTP/WS | Rust reqwest/tungstenite behind `remote_invoke` Tauri commands | Platform `fetch` + `WebSocket` directly (spec §3.7: no Tauri assumption on the wire) |
| Pairing | Code entry / URL | **QR scan** (primary gesture): `ralphx://pair?host=…#code=…` — one-time code in the hash fragment, exchanged for a per-device token |
| Scopes | `ui:read` + `ui:operate` default; `ui:agent` per-device host-side toggle | Identical — a phone granted `ui:agent` is a full controller; default pairing is "viewer with brakes" |
| WS auth | Short-lived ticket minted over the bearer, passed as query param | Identical (ticket flow was kept precisely for browser/mobile parity) |
| CORS | N/A (Rust-side) | Router's restrictive CORS already in place for non-proxied clients |

## 3. Tech stack — two viable options

### Option A (recommended): Tauri 2 mobile — maximum reuse

Tauri 2 ships iOS/Android targets. RalphX is already a Tauri app whose **client mode is a webview + a Rust transport proxy** — that entire architecture ports:

- **UI**: the existing React/TS frontend, unchanged component tree, with a mobile layout pass (navigation, touch targets, safe areas). The remote-environment code paths (transport router, `NetworkInvoke`/`NetworkEventBus`, environment supervisors, gate maps, reconnect UX) run as-is.
- **Core**: a slim build of the client-side Rust (`remote_invoke`/`remote_connect`/`remote_disconnect` commands, token custody, WS ownership, frame re-emission) compiled for mobile; host-mode/agent-spawning/PTY/git modules compiled out behind a feature flag (`client-only`).
- **Secure storage**: swap the macOS Keychain calls for the iOS Keychain / Android Keystore equivalents behind the existing token-storage trait.
- **Wins**: one codebase, the desktop's security posture preserved (bearer never in JS), every future desktop-client fix lands on mobile for free.
- **Risks**: Tauri mobile maturity (webview perf on long transcripts — the virtualized transcript work helps), app-store review of a webview-heavy app, iOS background-socket limits (see §5).

### Option B: React Native (Expo) — native feel, shared TS layer

- **UI**: React Native + Expo Router; NativeWind for Tailwind-parity styling.
- **Shared code**: extract the platform-agnostic TS into workspace packages consumed by both apps: `@ralphx/remote-protocol` (zod schemas, error taxonomy — the ten `REMOTE_*` codes, generated capability manifest), `@ralphx/remote-client` (invoke wrapper, event bus, cursor/epoch supervisor, intent-poll helpers), `@ralphx/api` (typed command wrappers). The React component tree is NOT shared; screens are rebuilt native.
- **Transport**: RN `fetch` + `WebSocket` directly implementing `NetworkInvoke`/`NetworkEventBus` — the seam pattern means only these two implementations are platform-specific.
- **Secure storage**: `expo-secure-store` (Keychain/Keystore).
- **Wins**: native navigation/gesture feel, better background/push integration, smaller runtime.
- **Costs**: rebuild every screen; permanent double-maintenance of UI behavior; the desktop webview never held the bearer, RN JS does (mitigate: keep token in SecureStore, mint per-request from a thin native module if desired).

Decision heuristic: if the goal is "manage the host from the phone, soon, with fidelity" → **Option A**. If the goal is a polished consumer-grade companion app → Option B, accepting the rebuild.

## 4. App architecture (either option)

```
┌─ UI (screens: Environments, Projects, Kanban, Conversation,
│      Reviews/Artifacts, Inbox, Approvals, Settings)
├─ Environment store        — N hosts + connection presentation state
├─ Per-environment supervisor — pairing, connect, ticket mint, retry/backoff,
│      epoch/cursor tracking, cold-hydrate orchestration (port of desktop TS supervisor)
├─ NetworkInvoke            — bearer invoke + typed REMOTE_* error envelope
├─ NetworkEventBus          — WS frames → local event emission (emit() is local-only)
├─ Query/cache layer        — TanStack Query keyed (envId, cmd, args);
│      event-driven invalidation exactly like the desktop remote env
├─ Intent helpers           — request→poll loops for start/continue/stop
└─ Native shims             — secure token storage, QR scanner, (later) push
```

Rules carried over from the desktop client, non-negotiable:
- `emit()` on the event bus is a local listener registry, never a WS write.
- Writable affordances require confirmed scopes AND live connection; scope snapshots from pairing are provisional, not confirmed.
- Every remote mutation carries `requestId` idempotency; resends are safe.
- Blocked-entry causes (401/403, version mismatch, malformed descriptor, invalid arguments) render as distinct terminal states, not generic errors.
- `MIN_CLIENT_PROTOCOL` is pinned independently host-side; the mobile client sends its protocol version and must render the version-mismatch refusal gracefully.

## 5. Mobile-specific deltas (the only genuinely new design work)

1. **Lifecycle**: iOS suspends sockets in background. Model it as the already-designed disconnect: on foreground, reconnect → same epoch ⇒ cursor resume; new epoch ⇒ cold-hydrate. No new protocol needed — mobile just hits the reconnect path more often.
2. **Push notifications** (post-v1): the host has a notification producer surface; a push bridge (APNs/FCM) is new host-side work — likely a relay decision, since the host has no cloud. Until then: badge-on-foreground from the notifications read surface.
3. **Attachment/media handling**: stream `/remote/v1/attachments/:id` to a file URI rather than blob-in-memory for large files.
4. **Battery/data**: coalesce event-driven invalidations (the desktop throttled-emitter pattern), prefer the paginated remote twins (`*_page` commands) everywhere.
5. **App-store posture**: the app executes nothing locally — it is a remote control. `ui:agent` consent lives on the HOST (per-device toggle), which is the right review story.

## 6. Phasing

1. **P0** — pairing (QR) + environment store + read plane: projects, kanban, transcripts, inbox, reviews (read). Ships against today's facade.
2. **P1** — brakes + gates at `ui:operate`: stop/pause/block, deny permission, question answering surfaces.
3. **P2** — full control at `ui:agent`: start/continue/steer conversations (intent rows), approvals, artifact + review management, options — i.e. everything the full-remote-management spec is landing now.
4. **P3** — push bridge, background polish, offline cache persistence across app restarts (needs snapshot+cursor atomically persisted — currently a spec non-goal, revisit).
