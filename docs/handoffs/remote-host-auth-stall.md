# Remote host: every authenticated route hangs — host-side handoff

**Status:** diagnosed from the client side, cause not yet confirmed on the host.
**Symptom (client):** a paired client shows `Connecting to "100.95.136.117:3849"… Setting up
this environment for the first time.` forever, and panes surface
`` `get_execution_settings` did not answer within 30000ms ``.
**Date:** 2026-07-31. Host: `reefs-mac-studio` (`100.95.136.117:3849`), RalphX 0.85.1.

---

## 1. What is measured, not guessed

All probes below are `curl` from the client Mac against the live host. The tailnet peer is
`active` and TCP connects in **0.02s**, so this is not a network problem.

| Route | Auth | Touches SQLite? | Result |
|---|---|---|---|
| `/.well-known/ralphx/environment` | pre-auth | **no** | **200**, repeatably |
| `/health` | bearer | yes (`resolve_device`) | **hangs** — no response in 25s |
| `/remote/v1/session` | bearer | yes | **hangs** |
| `/remote/v1/auth/ws-ticket` | bearer | yes | **hangs** |
| `/health` **without** a bearer | pre-auth reject | **yes** (`record_audit`) | **hangs** |

That last row is the decisive one. Yesterday an unauthenticated `/health` returned
`401 REMOTE_UNAUTHORIZED` instantly; today it hangs. The no-bearer path never looks up a
device — it goes straight to `reject(...)`, which **writes an audit row** before responding
(`remote_server/auth.rs:598-601` → `record_audit` → `remote_audit_log` INSERT).

So the split is not authenticated vs unauthenticated. It is:

> **Every request that touches the host's SQLite hangs. Every request that does not, answers.**

The descriptor handler reads no database, which is why it is the only thing still working.

## 2. Where it blocks

`authenticate_remote_request` (`remote_server/auth.rs:483+`) has exactly two DB sinks, and
both observed hangs land on one of them:

| Path | Sink | File |
|---|---|---|
| Bearer presented | `resolve_device` → `devices.lookup_by_token_hash` | `auth.rs:328` |
| No/!valid bearer | `reject` → `record_audit` → `audit.record` INSERT | `auth.rs:598`, `sqlite_remote_access_repo.rs:635` |

Ruled out by evidence, so do not start here:

- **Not the rate limiter.** `acquire_device_slot` returns `None` and rejects fast with
  `TooManyConcurrentRequests`; it cannot block. A saturated limiter would produce fast 403s,
  not silence.
- **Not the listener or router.** The descriptor is served by the same router, same port,
  same middleware stack minus the auth layer.
- **Not the client.** Same result from `curl` with no RalphX client involved.

## 3. Prime suspect: the DB is wedged

`DbConnection` is a `Mutex<Connection>` (single or small pool,
`infrastructure/sqlite/db_connection.rs:42-55`), and `run_transaction` uses `BEGIN IMMEDIATE`.
One long-held write transaction therefore stalls **every** other caller, which matches the
symptom exactly: not slow, but indefinitely silent.

Two writers this branch added run on a timer against that same database and are the first
things to look at:

1. **The retention pruner** — `retention::spawn_pruner`, every `PRUNE_INTERVAL` (5 min,
   `retention.rs:34`), deleting from `remote_event_log`. On a host that has accumulated a
   large log, a single unbounded `DELETE` inside one transaction is exactly the shape that
   parks everything else for minutes at a time.
2. **The durable sequencer** — micro-batched commits into `remote_event_log`
   (`sequencer.rs`), continuously while the host is emitting events.

Neither is proven. They are the two new periodic writers on the contended connection, so
they are where to look first.

## 4. Diagnostics to run ON the host

In rough order of cheapness:

1. **App log.** Grep the current launch log for `database is locked`, `SQLITE_BUSY`,
   `run_transaction`, `Remote audit log write failed`, and
   `Remote durable sequencer`. `record_audit` logs `Remote audit log write failed` on error —
   if the DB were erroring rather than blocking, that line would be present. Its **absence**
   alongside the hang is itself evidence of a block rather than a failure.
2. **Is the process healthy?** Confirm the running binary is the app and not a stale process
   holding `:3849` while a rebuild is in flight — TCP answering while HTTP does not is also
   consistent with that.
3. **Log-table size.** `sqlite3 ralphx.db "SELECT count(*) FROM remote_event_log;"` and
   `"SELECT count(*) FROM remote_audit_log;"`. A very large `remote_event_log` supports the
   pruner hypothesis. (Run this from a shell — if the DB is genuinely wedged this command
   will itself block, which is a positive result, not a failed command.)
4. **Wedged vs slow.** `sqlite3 ralphx.db ".timeout 2000" "SELECT 1;"` — if that cannot get
   through, the lock is held by the app process.
5. **Restart the host app.** If the DB was wedged, this clears it and the client reconnects
   on its next ladder tick with no client-side action. **This is also the immediate unblock**
   if you just need the pairing working again — but capture the log first, because a restart
   destroys the evidence.

## 5. Candidate fixes, once confirmed

Do not apply these blind; confirm §4 first.

| If | Then |
|---|---|
| The pruner holds a long transaction | Bound the delete — chunked `DELETE … LIMIT n` per tick, or a batched loop that releases the transaction between chunks. Retention is a background reclaim; it has no business blocking the auth path. |
| The sequencer's commit batching starves readers | Give the auth reads their own pooled connection, or shorten the commit batch window. Note WAL allows concurrent readers — a reader stalling implies a writer holding an exclusive lock, so check `BEGIN IMMEDIATE` scopes. |
| Neither reproduces under load | Add a `busy_timeout` at connection setup so a contended DB fails fast and typed instead of hanging forever. A request that returns `REMOTE_INTERNAL_ERROR` in 2s is strictly better than one that never returns: the client supervisor can classify it, the user sees a real error, and the 15s connect budget stops being consumed by silence. |

## 6. The client-side hardening this exposes (separate lane, not host work)

Worth recording because the client behaved poorly against a silent host:

- The supervisor spends its **entire 15s connect budget** on a host that accepts TCP and
  then says nothing, because step 1 (descriptor) *succeeds* and only step 2 hangs. It never
  reaches `blocked`, so the UI says "Connecting… first time" indefinitely with no hint that
  the host is unhealthy rather than absent.
- A host that answers the descriptor but stalls every authenticated route is a distinguishable
  state and deserves distinct copy — "the host is not responding" rather than "connecting".
- `REMOTE_TIMEOUT_UNKNOWN` surfaced raw to the user (`get_execution_settings did not answer
  within 30000ms. The outcome is UNKNOWN — reconcile by refetching, do not re-send.`). That
  is protocol-accurate and user-hostile; it should render as a plain "the host stopped
  responding" with a retry affordance.

---

## Appendix — reproducing the measurement from any client

```bash
HOST=http://100.95.136.117:3849
TOKEN=$(security find-generic-password -s com.ralphx.app \
  -a "remote-env:<environment-row-id>:token" -w)

# Answers (no DB):
curl -s -m 10 -o /dev/null -w "descriptor %{http_code} %{time_total}s\n" \
  "$HOST/.well-known/ralphx/environment"

# Hangs (DB read):
curl -s -m 10 -o /dev/null -w "health   %{http_code} %{time_total}s\n" \
  -H "Authorization: Bearer $TOKEN" "$HOST/health"

# Hangs (DB write on the reject path) — the decisive probe:
curl -s -m 10 -o /dev/null -w "no-auth  %{http_code} %{time_total}s\n" "$HOST/health"
```

A healthy host answers all three: `200`, `200`, and `401` respectively.
