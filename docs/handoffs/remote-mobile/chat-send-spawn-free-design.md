# Chat send on the remote facade — a spawn-free seam

**Status:** design only, no implementation. Written for Fable/owner review.
**Lane:** PR 3.1-b batch 2 · **Base:** `feat/rme-31b-batch2` @ `2774540e4`
**Unblocks:** the single most user-visible remote gap — a paired `ui:agent` device cannot
send a chat message, because `send_agent_message` is unregistered and the client reads
`REMOTE_COMMAND_UNAVAILABLE` as "this host does not support chat".

---

## 1. Why this is not just "register it at ui:agent"

The obvious move — ledger `send_agent_message` as `AgentControl`, register it, require
`ui:agent` — is already what the ledger says, and it is still not registrable. The blocker is
not the risk class. It is that **the command spawns a provider process**, and the facade has a
standing structural rule that no registered command may:

> `detector (c)`: reaching a `tool_paths` resolver IS spawning a process; `SpawnsProcess` is
> expressible only under `Elevated` (`capability_ledger_tests::detector_c_floors_process_spawn_authority`).

`detector_c_floors_process_spawn_authority` asserts `registered_spawners.is_empty()`. So the
gate that stops chat send is a *capability* gate, not a scope gate, and no amount of scope
tightening opens it. Something has to change about the command's reachable sinks.

That framing matters for the owner decision: this is not "do we trust remote users with
chat", it is "where does the process start, and who is standing there when it does".

---

## 2. What the audit engine actually reports

Run reproducibly via the checked-in probe:

```
cargo test --features test-utils --lib \
  remote_server::capability_ledger_tests::probe_chat_send_trio_sink_paths \
  -- --ignored --nocapture
```

Detectors: (a) transitively reaches an agent spawn/steer sink, target-sensitive; (b) writes
persisted state a registered background loop consumes to spawn/steer; (c) resolves a CLI path.

| Command | Module | a | b | c | Class today |
|---|---|:-:|:-:|:-:|---|
| `send_agent_message` | `unified_chat_commands` | ✅ | ✅ | ✅ | AgentControl |
| `start_agent_conversation` | `unified_chat_commands` | ✅ | ✅ | ✅ | AgentControl |
| `create_agent_conversation` | `unified_chat_commands` | — | — | — | AgentControl *(module default)* |
| `send_chat_message` | `ideation_commands` | — | — | — | AgentControl *(module default)* |
| `resume_automation` | `automation_commands` | ✅ | ✅ | — | AgentControl `SeedsSpawnTriggeringState` |
| `finalize_automation` | `automation_commands` | — | ✅ | — | AgentControl `SeedsSpawnTriggeringState` |
| `stop_automation` | `automation_commands` | — | — | — | AgentControl *(module default)* |
| `pause_automation` | `automation_commands` | — | — | — | AgentControl *(module default)* |

**Exact sink paths** (the `PROBE-TRIO` lines, not paraphrased):

```
send_agent_message       STEER  -> ["send_message"]
send_agent_message       LAUNCH -> ["crate::infrastructure::tool_paths::find_codex_cli_candidates",
                                    "crate::infrastructure::tool_paths::resolve_node_cli_path",
                                    "find_codex_cli_candidates", "resolve_git_cli_path",
                                    "resolve_node_cli_path"]

start_agent_conversation STEER      -> ["send_message", "write_message"]
start_agent_conversation SCHEDULER  -> ["execute_entry_actions", "try_schedule_ready_tasks"]
start_agent_conversation TRANSITION -> ["transition_task", "transition_task_corrective_with_exit"]
start_agent_conversation LAUNCH     -> (same five resolvers)

resume_automation        STEER  -> ["send_message"]
```

Two readings worth stating plainly:

- `send_agent_message` carries **strictly less** authority than `start_agent_conversation`. It
  reaches steer + launch, but NOT the scheduler and NOT the task transition sinks. The two are
  routinely discussed as one problem; they are not one problem, and the cheaper half is the one
  users actually want.
- `start_agent_conversation` reaching `transition_task` and `try_schedule_ready_tasks` means
  starting a conversation can move a *task* through the execution state machine. Any remote
  exposure of it is remote control of the Kanban pipeline, not just of a chat pane.

---

## 3. The finding that shapes the design: the seam already exists

The spawn-free seam does not have to be invented. **RalphX already ships one, on the ideation
chat surface**, and the probe found it by accident:

`ideation_commands::send_chat_message` is detector-silent on all three because its entire body
is validate-then-persist — `chat_message_repo.create(message)` is the last statement
(`ideation_commands/ideation_commands_chat.rs`). It starts no process, emits no event, and
steers nothing. Agent invocation on that surface is a separate concern from message
persistence.

`unified_chat_commands::create_agent_conversation` is likewise detector-silent: creating the
conversation row is already separable from starting it.

So the codebase's own answer to "can a chat send be spawn-free" is **yes, and here is the
established pattern** — persistence-only send is not a novel architecture this PR would
introduce. That materially lowers the risk of the proposal below, and it should be the first
thing a reviewer checks, because it converts the question from "design a new seam" into
"extend an existing one to a second surface".

### 3.1 …but `send_chat_message` must NOT simply be registered

It is detector-silent and it is still **not** a safe registration as written. It takes a
client-supplied `role` and will persist a message as `Orchestrator`, `Worker`, `Reviewer`, or
`Merger`:

```rust
let role: MessageRole = input.role.parse()...;
match role {
    MessageRole::User => ChatMessage::user_in_session(...),
    MessageRole::Orchestrator => ChatMessage::orchestrator_in_session(...),
    ...
}
```

Those rows are agent-consumed transcript context. A remote client that can write a message
attributed to the *orchestrator* can put words in the agent's own mouth — prompt injection with
a forged speaker label, which is worse than plain content injection because the transcript's
role field is exactly what downstream prompt assembly trusts to distinguish instruction from
user input.

This is a `MutatesAgentConsumedContent` surface with a **role-spoofing** amplifier, and it is
the concrete reason the seam needs a server pin rather than a bare registration. The facade
already has the mechanism: `PinnedField` (`registry.rs`), which `deny_permission_request` uses
to pin `decision = "deny"` so a client sending `"allow"` still denies.

**Proposal A (cheap, high value, low risk):** register `send_chat_message` with
`pins: [PinnedField { param: "input", field: "role", value: "user" }]`. A remote client then
cannot author anything but a user turn, by construction, on the wire path — pins are read from
`spec.pins` at dispatch time, so the declaration cannot drift from behaviour.

---

## 4. Per-op analysis and proposal

### 4.1 `send_agent_message` — the one users want

Reaches steer + launch because the chat service's `send_message` will **start a provider
process if no live session is attached**. The send and the spawn are fused in one command.

Three candidate seams, in increasing cost:

**Option 1 — Queue-then-authorize (recommended for design review).**
Split the command at the fusion point:

- `enqueue_agent_message` — persists the user turn plus a "pending dispatch" marker, exactly
  the `send_chat_message` shape. Detector-silent by construction. Registrable at `ui:agent`
  with a pinned role.
- Dispatch to the provider happens on the HOST side, driven by the existing chat service, not
  by the remote request.

The honest problem, and the reason this is a *design* question rather than a task: **what
drives the dispatch?** If a background loop drains the queue, detector (b) will flag
`enqueue_agent_message` as a spawn-triggering writer the moment that loop is registered in the
loop inventory — and correctly so, because the remote client's write would then cause a spawn.
The capability would become `SeedsSpawnTriggeringState`, which IS expressible under
`AgentControl` (`inject_task` and `resume_automation` already carry it). That is a *coherent*
outcome, not a defeat: it means remote chat send is ledgered as "seeds state a scheduler
consumes", the same class as injecting a task, and it never resolves a CLI path on the request
path. Detector (c) stays silent, which is the gate that currently blocks registration.

So Option 1's real claim is: **move the process launch off the request path**, accept
`SeedsSpawnTriggeringState`, and let the existing floor machinery classify it honestly.

**Option 2 — Live-session-only send.** Register a narrowed command that refuses when no live
provider session exists (`REMOTE_*` error rather than starting one). Sending to an already-running
conversation is then genuinely spawn-free. Cheaper than Option 1 and needs no queue, but the
UX is conditional in a way remote users will find arbitrary ("send works, except when it
doesn't"), and it needs a fail-closed liveness read — if the "is there a session" check errors
and is treated as "no session", the user gets a confusing refusal; if treated as "yes", the
command spawns and the gate is defeated. That check is the whole security surface.

**Option 3 — Elevated registration.** Ledger it `Elevated` + `SpawnsProcess` and relax
`detector_c_floors_process_spawn_authority` for it. **Not recommended.** It converts the
facade's strongest mechanical invariant into a per-command judgement call, and the first
exception is the one that makes the second cheap. It also puts chat send behind `ui:elevated`,
which is not the scope the product wants for the default remote pairing.

### 4.2 `start_agent_conversation` — recommend NOT registering in 3.1

It reaches scheduler AND transition AND launch. Registering it is remote control of the task
state machine under a chat-shaped name. The user-facing need ("I want to chat from my phone")
is served by sending into a conversation the host already started; **starting** one remotely is
a separate product decision that should not ride a registration sweep.

If it is wanted later, `create_agent_conversation` (detector-silent) is the registrable half —
create remotely, start on the host — which is the same split as Option 1.

### 4.3 The 2.6-surfaced automation ops

`stop_automation` and `pause_automation` are detector-silent and are **authority-reducing** in
the exact sense the ledger already recognises (`pause_task`, `block_task`, `stop_task`,
`deny_permission_request` all carry `AuthorityReducingExemption` down to `Operate`).

**Proposal B:** audit both for the exemption and, if they hold, ledger them `Operate` with an
`AUTHORITY_REDUCING_EXEMPTIONS` row and register at `ui:operate`. This extends the product's
promised "viewer with brakes" boundary to automation, which today can be started from a phone's
view of the board but not stopped. That asymmetry is the worst possible one to ship.

`resume_automation` and `finalize_automation` stay `AgentControl` — both already carry
`SeedsSpawnTriggeringState` from a real detector (b) hit, and `resume_automation` additionally
reaches `send_message`. Restoring authority is not the mirror of reducing it.

---

## 5. Recommended sequencing

| # | Change | Class / scope | Cost | Risk |
|---|---|---|---|---|
| 1 | `send_chat_message` + pinned `role: "user"` | AgentControl / `ui:agent` | low | low — detector-silent, pin is proven machinery |
| 2 | `stop_automation`, `pause_automation` if the brake audit holds | Operate / `ui:operate` | low | low — established exemption shape |
| 3 | `send_agent_message` via Option 1 queue seam | AgentControl `SeedsSpawnTriggeringState` / `ui:agent` | high | medium — new dispatch driver, needs loop-inventory row |
| 4 | `start_agent_conversation` | — | — | defer past 3.1 |

Items 1 and 2 are registration-shaped and could land in a batch-3 sweep. Item 3 is a feature
with a state machine and belongs in its own PR.

---

## 6. Owner decisions required

1. **Is remote chat send allowed to cause a provider process to start at all?** If yes,
   Option 1 is the design and `SeedsSpawnTriggeringState` is the honest capability. If no, only
   Option 2 (live-session-only) is available and the UX is conditional. Everything else follows
   from this answer.
2. **Should `detector_c_floors_process_spawn_authority` ever admit an exception?** The
   recommendation is a firm no, and to treat any command that needs one as a redesign signal.
   Worth an explicit ruling, because Option 3 will keep being proposed as the cheap path.
3. **Is remote *starting* of conversations/tasks in scope for the remote product at all**, or
   is remote confined to participating in work the host began? §4.2 and the deferral of
   `start_agent_conversation` assume the latter.
4. **`ui:agent` vs a new scope for content authorship.** Every proposal here writes
   agent-consumed content. The scope set has no "may write transcript content" distinct from
   "may steer an agent". If the owner wants a device that reads and brakes but cannot author
   prompt text, that is a new scope, and deciding it before item 1 lands is much cheaper than
   after.
5. **Does the pinned-role restriction break a real client need?** Item 1 assumes no legitimate
   remote client authors non-user roles. If the mobile client is ever meant to replay
   orchestrator turns, the pin is wrong and the design needs a different guard.

---

## 7. What this design does NOT claim

- No claim that Option 1's queue is free of the false-success class. A queued message that is
  persisted but never dispatched shows the user a sent message that no agent ever saw. The
  dispatch driver needs its own terminal/failure surfacing, and that is a substantial part of
  item 3's cost — it is exactly the "authority before effects" rule in
  `stateful-workflow-review.md` and should be reviewed under that lens, not this one.
- No claim that the detector silence of `send_chat_message` / `create_agent_conversation` makes
  them safe. Detectors are a floor. §3.1 is a live example of a detector-silent command that is
  unsafe to register as written, and it was found by hand-tracing, not by the engine.
- No audit of the remaining `unified_chat_commands` / `automation_commands` members. Only the
  commands listed in §2 were probed.
