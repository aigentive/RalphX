# Delegate-Signalled Coordinator Wakeups

RalphX coordinators can wait for their native delegates without repeatedly polling or holding an execution slot for the delegate's whole runtime. Choose a short backend-held wait when the next result is imminent; park the coordinator when the wave may take longer.

## Behavior

1. A coordinator starts one or more RalphX-native delegate jobs.
2. For a short wait, it calls `delegate_wait` with the jobs to watch and a `wait_timeout_ms` cap.
3. For a long-running wave, it calls `delegate_park` with the outstanding jobs and wake policy, then ends its turn.
4. Delegate settlement, a deadline, or the selected failure policy makes the parked coordinator eligible to wake.
5. RalphX queues a hidden `resume_in_place` message in the same conversation.
6. The resumed coordinator inspects its delegates and continues the normal workflow.

Omitting `wait_timeout_ms` preserves `delegate_wait`'s immediate-return behavior. With it, RalphX holds the wait in the backend until a watched job settles or the cap expires, then returns the result (including `timed_out: true` on timeout). A single call may watch a whole wave with `job_ids[]`.

## Waiting modes

| Mode | Use it when | Result |
|---|---|---|
| Bounded `delegate_wait` | A delegate result is likely soon and the coordinator should continue in the same turn. | The backend returns as soon as any watched job settles, or at the requested timeout; it never requires a model-side polling loop. |
| `delegate_park` + turn end | Delegate work may take minutes and the coordinator has no useful work until it settles. | The coordinator releases its execution slot; RalphX wakes the same conversation later with a hidden `resume_in_place` message. |

The backend enforces a maximum block duration. Its ceiling is kept below the stream parse-stall guard, so a legitimate bounded wait cannot be misclassified as a stalled coordinator stream.

## Park lifecycle and wake policy

Park records move through `armed`, `waking`, `woken`, `superseded`, `expired`, or `failed`.

`delegate_park` watches the supplied outstanding job ids with one of these policies:

| Policy | Effect |
|---|---|
| `all` (default) | Wake after every watched delegate has settled. |
| `any` | Wake after the first watched delegate settles. |
| `wake_on_failure` | Wake immediately when a watched delegate fails or is cancelled, independent of the normal `all` or `any` condition. |

Every park has a deadline. If no policy condition is met first, RalphX force-wakes the coordinator with an explicit timeout notice.

## Durable safety

Parks and their watched-job links are durable, generation-scoped records. RalphX preserves these boundaries when dispatching a wake:

- A delegate can wake its coordinator only after that delegation's terminal commit compare-and-swap has accepted.
- Wake dispatch claims the park with an `armed` → `waking` compare-and-swap, so a park injects at most one wake message.
- A user message to the parked conversation supersedes the park and prevents a later hidden wake from being injected.
- If the conversation started a different run *after* the park was armed, the park becomes `superseded` instead of injecting into a stale run. The timing rule matters: a `running` row that predates the park cannot represent the conversation moving on, so an orphaned row left behind by a killed process no longer suppresses a legitimate wake.
- A read failure at the parent-settlement gate fails closed: the delegate remains pending rather than being settled while its coordinator may still be parked.
- A coordinator that is itself a delegate does not settle its own parent job while its park remains armed.
- A coordinator may only park on jobs its own current run started, and that run must belong to the parking conversation. Ownership is proven from the caller run rather than the job's parent conversation, because nested delegates and ideation verification children record an ancestor conversation there as the Delegate widget's lineage anchor.

## Restart and recovery

Startup reconciliation examines durable armed parks and already-settled delegates. It dispatches any wake that was due while RalphX was not running, using the same guarded claim path as a live settlement. This keeps restart recovery aligned with live behavior and avoids duplicate wake messages.

## Settings

The top-level `delegation:` section in `config/ralphx.yaml` controls the feature. Each setting has a matching `RALPHX_DELEGATION_*` environment override.

| Setting | Default | Effect |
|---|---:|---|
| `wait_block_secs` | 120 | Default backend-held wait duration. |
| `wait_block_max_secs` | 150 | Maximum allowed backend-held wait duration. |
| `park_max_secs` | 3600 | Maximum lifetime of a park before its timeout wake. |
| `park_wake_retry_max` | 5 | Maximum retries for dispatching a wake. |
| `park_wake_retry_backoff_secs` | 30 | Delay between wake-dispatch retries. |

## Sidebar presentation

A coordinator with an armed park remains in the Agents sidebar's `working` lane as `Waiting on N delegates`. It does not fall into **Needs you** merely because its turn has ended; the displayed state reflects that RalphX is still awaiting its delegate wave.
