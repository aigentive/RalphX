# RalphX Remote Protocol (v1)

Developer-facing contract for the RalphX remote host surface — the wire a client environment
(desktop client mode today, the mobile app next) speaks to a RalphX host.

This document is the **cross-spec contract**. A client written against it should need no
knowledge of RalphX internals. Where this document and older spec text disagree, this document
and `docs/handoffs/remote-mobile/spec-amendment-proposal.md` are authoritative — the amendments
record the places the implementation is right and the original spec prose is stale.

Everything here ships dark behind the backend `remote_host` settings row and the frontend
`remoteEnvironments` flag.

---

## 1. Transport and versioning

| Property | Value |
|---|---|
| Default port | `3849` (`:3847` is the local-only backend and is never remote-reachable) |
| Bind policy | loopback, or a validated tailnet address — never `0.0.0.0` |
| Protocol version | `PROTOCOL_VERSION = 1`, advertised in the descriptor and in `hello` |
| Client floor | `MIN_CLIENT_PROTOCOL = 1`, an **independent** constant |

`MIN_CLIENT_PROTOCOL` is deliberately *not* aliased to `PROTOCOL_VERSION`. Raising the floor
refuses every already-shipped client at the descriptor gate, so it is a deliberate
compatibility decision that must be argued on its own terms — never a side effect of bumping
`PROTOCOL_VERSION`. Evolution is additive (R-7): new frames, fields, and commands may appear at
version 1; removals or renames require a floor raise.

### 1.1 Endpoint table

| Method | Path | Auth | Purpose |
|---|---|---|---|
| GET | `/.well-known/ralphx/environment` | **pre-auth** | Descriptor: environment id, protocol version, min client protocol, server version, platform |
| POST | `/remote/v1/auth/pair` | **pre-auth** | Exchange a pairing code for a long-term device token |
| POST | `/remote/v1/auth/ws-ticket` | bearer | Mint a single-use WS ticket |
| POST | `/remote/v1/auth/revoke` | bearer, self-scoped | Device revokes itself; identity comes from the middleware, there is no device-id argument |
| GET | `/remote/v1/session` | bearer | Confirmed scope introspection for the calling device |
| POST | `/remote/v1/invoke` | bearer | The command facade |
| GET | `/remote/v1/events?ticket=…` | ticket | WebSocket event stream |
| GET | `/health` | bearer | Liveness |

Two properties are load-bearing and CI-enforced:

- **The pre-auth allowlist has exactly two entries** — descriptor and pairing. Everything else,
  including `/health`, runs behind the bearer check. There is no zero-devices bootstrap
  exception (A-2): a host with no paired devices still authenticates every other route.
- **`/remote/v1/events` is ticket-authenticated, not pre-auth.** A browser cannot attach an
  `Authorization` header to a WebSocket upgrade, so the client mints a single-use, device-bound
  ticket over the bearer-authenticated route first. It is tracked in a separate allowlist so the
  "exactly two pre-auth routes" guarantee stays literally true.

A curated set of read-only `:3847` fetch routes is remounted on `:3849` behind the same auth and
scope middleware, reusing the same handler functions. The remount list is a checked-in
allowlist; route-set equality is asserted in CI. Named-denied sinks (`/api/permission/resolve`,
`/api/question/resolve`, `/api/add_task_note`) stay unmounted.

---

## 2. Authentication and authorization

### 2.1 Pairing

The host mints a short-lived pairing code. The client posts the code plus a device name, its
client version, and the scopes it requests. The host **intersects** the request with the code's
grant and issues a long-term device token (`rxd_live_…`).

Pairing codes are single-use: the second exchange of the same code is refused.

### 2.2 Scopes

| Scope | Meaning |
|---|---|
| `ui:read` | Read the workspace |
| `ui:operate` | Global pause/stop brakes, attachment handling, and low-risk edits |
| `ui:agent` | Start, resume, restart, or steer an agent |
| `ui:elevated` | **Reserved, not implemented.** Placeholder for the deferred terminal/PTY surface |

The default pairing grant is `ui:read + ui:operate`. **`ui:agent` is not grantable at pairing
time** — the pairing-grant validator refuses it. It is a separate, off-by-default, per-device
toggle. See §6 and `docs/features/remote-access.md` for why that separation exists.

### 2.3 Capability classes

Every registered command carries a hand-audited risk class. The class mechanically determines
the scope required (`scope_for_class`); the scope is never declared independently, so the two
cannot drift.

| Class | Required scope | Meaning |
|---|---|---|
| `Read` | `ui:read` | No downstream authority |
| `Operate` | `ui:operate` | Brakes and low-risk mutations |
| `PathScoped` | `ui:operate` | Operate, plus a path-containment predicate |
| `AgentControl` | `ui:agent` | Can start or steer an agent, directly or by seeding state a background loop consumes |
| `Elevated` | `ui:elevated` | Spawns processes / touches credentials — unreachable in v1 |
| `Denied` | — | **Unregistrable.** Registering one fails compilation |

Classes are backed by an eleven-member capability vocabulary: `spawnsProcess`,
`writesArbitraryPath`, `mutatesWorkingDirectory`, `configuresFutureProcessAuthority`,
`touchesCredentials`, `ptyControl`, `agentControl`, `seedsSpawnTriggeringState`,
`mutatesAgentConsumedContent`, `hostManagement`, `deletesEntity`. A capability that the declared
class does not permit is a **compile error**, not a lint.

Two audit rules matter to a client author:

- The classification traces *downstream* authority, not immediate action. A command that merely
  writes a database row is `AgentControl` if a background loop turns that row into a spawn.
- The default-tier brakes are deliberately narrow: `pause_execution` and `stop_execution` stay
  `ui:operate` because they set the process-wide pause gate before any task transition.
  `deny_permission_request` also stays `ui:operate` and server-pins `decision = "deny"`, so a
  client that sends `"allow"` still denies. Per-task `block_task`, `pause_task`, and `stop_task`,
  plus bulk `pause_tasks_in_group` and `cancel_tasks_in_group`, require `ui:agent`: agent-active
  exits can run Git side effects, and `block_task` can free capacity and ask the scheduler to
  launch queued work.

### 2.4 The manifest

`docs/generated/remote-commands.json` is the generated, diff-checked manifest: every registered
command with its risk class and capability set, plus six audit tables (loop inventory, state
surface, content surface, `WorkerTaskView` allowlist, exemption table, declared memberships).
Staleness fails CI.

Two documented reductions from the originally specified schema: `scope` is omitted because it is
mechanically derivable from `riskClass` and duplicating it would invite drift; `argNames` is
omitted because the wire argument surface is guarded by the frontend AST census (P-11) instead.
A consumer that needs `argNames` should request it as a schema addition rather than infer it.

### 2.5 Revocation

Revocation is durable-first and takes effect on **live** sessions, not just future requests: the
existing WebSocket closes with `error{revoked}` within the heartbeat window, and the next bearer
request 401s. A client must treat both as the same event.

---

## 3. The command facade

```
POST /remote/v1/invoke
Authorization: Bearer rxd_live_…
{ "requestId": "<uuid>", "cmd": "list_tasks", "args": { … } }
```

The facade is an **allowlist**, never a passthrough. Commands are registered one at a time in
`remote_server/registry.rs` against the existing local Tauri command functions — there are no
handler forks and no `generate_handler!` edits, so remote and local dispatch cannot diverge.
Serialization is byte-identical to local Tauri IPC across argument shapes and error paths.

`requestId` is client-minted and binds mutation dedup: replaying the **same** id returns the
cached outcome instead of executing twice. A client retrying a mutation must reuse the id; a
client starting new work must mint a new one.

### 3.1 Error taxonomy — exactly ten codes

| Code | HTTP | Retryable | Meaning |
|---|---|---|---|
| `REMOTE_UNAUTHORIZED` | 401 | no | Missing, revoked, or invalid credential |
| `REMOTE_FORBIDDEN` | 403 | no | Authenticated, but the device lacks the required scope |
| `REMOTE_COMMAND_UNAVAILABLE` | 404 | no | Not in the allowlist |
| `REMOTE_INVALID_ARGUMENTS` | 400 | no | Argument shape rejected — an identical resend cannot succeed |
| `REMOTE_VERSION_MISMATCH` | — | no | Client below `MIN_CLIENT_PROTOCOL` |
| `REMOTE_REQUEST_IN_PROGRESS` | — | yes | Same `requestId` still executing |
| `REMOTE_REQUEST_ID_REUSED` | — | no | `requestId` reused for different arguments |
| `REMOTE_UNREACHABLE` | — | yes | Transport failure |
| `REMOTE_TIMEOUT_UNKNOWN` | — | **unknown** | Outcome indeterminate — retry only with the same `requestId` |
| `REMOTE_INTERNAL_ERROR` | 500 | yes | Host-side failure, distinct from transport failure |

`REMOTE_TIMEOUT_UNKNOWN` is the only code where the client must not assume the mutation did *or*
did not happen. Resolve it by replaying the same `requestId`.

Four causes move a client environment to `blocked` rather than `reconnecting`: 401/403, version
mismatch, malformed descriptor, invalid arguments.

---

## 4. The event stream

### 4.1 Frames

Server → client:

| Frame | Fields | Purpose |
|---|---|---|
| `hello` | `protocolVersion`, `environmentId`, `streamEpoch`, `serverVersion`, `maxSeq`, `heartbeatSecs` | First frame after upgrade |
| `event` | `seq` (**optional**), `name`, `payload` | An application event |
| `replayDone` | `throughSeq` | Cursor replay finished |
| `reset` | `reason` | Warm resume is impossible; cold hydrate |
| `heartbeat` | `t` | Liveness probe |
| `error` | `code`, `message` | Terminal error for this session |

Client → server: `subscribe{afterSeq, streamEpoch}`, `cursorAck{seq}`, `heartbeatAck{t}`.

Heartbeat cadence is 20 s and the session closes after two unacked beats, so a dead host or a
half-open socket is detected in roughly 40 s.

### 4.2 Delivery classes

Every event name is classified in a checked-in table with one of three deliveries:

- **Durable** — sequenced, persisted to `remote_event_log`, replayable by cursor.
- **Transient** — broadcast only, **bypassing the sequencer entirely**. Carries **no `seq`**.
- **LocalOnly** — never leaves the host.

The `seq: null` on a transient frame is a contract, not an accident: a transient frame must
never advance a client's resume cursor. High-volume streaming (`agent:chunk`,
`agent:usage_updated`) is Transient and is **never** written to `remote_event_log`. Resume for
those surfaces is owned by snapshots and fetch routes, not by replay — a client that reconnects
mid-turn repairs the missing chunk text by re-fetching the message, not by asking for chunks
again.

`agent_terminal:event` is the single `excluded_from_v1` class. No PTY route is mounted, no
terminal command is registered, and the terminal drawer is hidden for remote environments.
Re-enabling it requires the deferred `ui:elevated` scope.

### 4.3 The `H` barrier and the canonical resume rule

`streamEpoch` identifies one contiguous run of the durable log. It resets per host boot and rolls
live under overload. **A cursor is only meaningful within its epoch.**

Cold hydrate:

1. Connect; read `H = hello.maxSeq`.
2. Fetch snapshots (the REST/fetch surface) for the state you need.
3. `subscribe{afterSeq: H, streamEpoch}`.
4. Apply events from `H+1` onward.

Taking `H` **before** fetching snapshots is what makes this safe: any event that lands during the
fetch is above `H` and therefore replayed, so nothing is lost. Events at or below `H` are already
reflected in the snapshot, so nothing is double-applied.

Warm resume: `subscribe{afterSeq: lastCommittedSeq, streamEpoch}`, apply the replay, then treat
`replayDone` as the point where live delivery resumes.

The client must **cold hydrate**, not splice, whenever it receives `reset`:

| Reason | Cause |
|---|---|
| `cursor_pruned` | Retention advanced past the cursor — the missed rows no longer exist |
| `epoch_changed` | The epoch rolled; the old cursor addresses a different log |
| `after_seq_gt_max` | The client's cursor is ahead of the host (host rollback / different host) |
| `read_error` | The host could not read the durable range — **fail closed**, never treated as "no rows" |
| `revoked` | Credential revoked |
| `host_disabled` | Host mode turned off |

`read_error` deserves emphasis: an empty replay and a failed replay are different, and a client
that conflates them silently drops events.

### 4.4 Sequencer, leases, and retention

A single-actor sequencer allocates seqs, micro-batches commits, then publishes. Publication
happens strictly **after** the commit, so a frame a client has seen is always durable.

Under overload the sequencer **rolls the epoch live** rather than blocking emitters. Connected
clients get `reset{epoch_changed}` and cold hydrate. Blocking the application to preserve a
remote client's cursor is never the right trade.

Retention is `max(50 000 rows, 7 days)`. A subscribed client holds a **retention lease** at its
acked cursor; the pruner deletes only rows at or below the minimum live lease cursor. A client
that stops acking has its lease TTL-expire, after which the pruner is free to advance past it —
and its next interaction yields `reset{cursor_pruned}`.

`cursorAck` means **committed**, not merely received. A background observer that counts events
without projecting them must therefore **not** ack: acking would mint a resume cursor with no
commit behind it. Background environments let the lease expire and absorb the resulting
`cursor_pruned` as ignorable, because reactivation is always a full cold hydrate anyway.

### 4.5 RS-EXT-1 — the canonical forwarder source

Any external push forwarder (mobile push, webhooks) **must** tap the classified capture /
sequencer broadcast stream. It must never re-derive event sources from `EventSink` or
`AppState.events`.

The classified stream is the only place where the delivery classification, the v1 exclusions, and
the durable ordering have all been applied. A forwarder that reads the raw sink bypasses all
three and will eventually forward a `LocalOnly` event, a terminal byte, or an unordered
duplicate.

---

## 5. Client-side model

### 5.1 Supervisor FSM

One supervisor per environment owns the connection lifecycle:

```
idle → connecting → connected → degraded → reconnecting → connected
                        ↓                       ↓
                     blocked ←──────────────────┘
```

`blocked` is terminal-until-user-action and is entered only for the four causes in §3.1. It is
deliberately distinct from `reconnecting`: retrying a 403 forever is a bug, not resilience.

### 5.2 Environment isolation

Environments are isolated by construction: per-environment QueryClient, per-environment event
bus, per-environment cursor. Two invariants are enforced by tests:

- **An inactive environment never advances its warm cursor and never mutates its cache.** Any
  observation it does (badge counts) is observation only; reactivation is always a full
  `H`-barrier cold hydrate.
- **A background environment issues health-only operations** (descriptor probe, heartbeat)
  through the Rust proxy. It may not issue arbitrary invokes.

The local environment is never affected by remote flapping.

### 5.3 Mobile non-preclusion

Nothing in this protocol assumes a Tauri client. The facade is plain HTTP + JSON, the stream is a
plain WebSocket, and no path rides `window.__TAURI_INTERNALS__`. A mobile client consumes §1.1,
§2, §3.1, and §4 verbatim.

---

## 6. Security boundary

- `:3847` (the local backend) is **loopback-only and byte-identical** to its non-remote form. No
  `:3847` trust-header handler is reachable on `:3849`: presenting `X-RalphX-Tauri-MCP: 1` to the
  remote listener yields 401, and trust headers are stripped at the remote router's edge.
- CORS on `:3849` admits only the shipped app origins.
- The command facade is an allowlist; the fetch remount is an allowlist; the pre-auth surface is
  two routes. All three are asserted as *equalities* in CI, so an accidental addition fails.
- `ui:agent` is the real trust boundary. See `docs/features/remote-access.md` §"What you are
  actually granting" — a stolen `ui:agent` token lets the holder run code on the host machine.

---

## 7. Where the code lives

| Concern | File |
|---|---|
| Router, routes, allowlists | `src-tauri/src/remote_server/mod.rs` |
| Pairing, tokens, revocation | `src-tauri/src/remote_server/auth.rs`, `auth_endpoints.rs` |
| Command allowlist | `src-tauri/src/remote_server/registry.rs` |
| Risk classes and capabilities | `src-tauri/src/remote_server/capability_ledger.rs` |
| Event classification + capture | `src-tauri/src/remote_server/capture.rs`, `crates/ralphx-remote-protocol` |
| Sequencer, epoch, publication | `src-tauri/src/remote_server/sequencer.rs` |
| Leases, prune, retention | `src-tauri/src/remote_server/retention.rs` |
| WS sessions, heartbeat, replay | `src-tauri/src/remote_server/ws.rs` |
| Fetch remount allowlist | `src-tauri/src/remote_server/fetch_remount.rs` |
| Two-instance test fixture | `src-tauri/src/remote_server/harness.rs` (`test-utils`) |
| E2E suite | `src-tauri/tests/remote_e2e.rs` |
| Client transport / bus / supervisor | `frontend/src/lib/remote/**` |
