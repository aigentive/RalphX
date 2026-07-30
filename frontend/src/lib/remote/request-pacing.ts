/**
 * Client-side pacing for outbound remote proxy calls.
 *
 * The host enforces, per device: a 10-token bucket refilling at 10/s and 8
 * concurrent request slots (`rate_limit.rs`, `REMOTE_RATE_LIMIT_DEFAULTS`), and
 * every refusal costs a durable audit row. The hydration barrier refetches every
 * mounted query at once, so an unpaced client bursts 30+ concurrent calls,
 * bounces off both limits (`REMOTE_FORBIDDEN` "Too many remote requests"), fails
 * the barrier, redials, and repeats against an already-drained bucket — a
 * self-sustaining reconnect storm. Pacing below the budget makes the same sweep
 * drain instead of bounce.
 *
 * Budget fit: 6 in flight (< 8 slots) and one start per 110ms (~9/s, < 10/s
 * refill; the full bucket absorbs the first burst). A 40-query hydrate drains in
 * ~5s — inside the supervisor's 15s connect budget. Local calls never pass
 * through here; only the remote proxy seams (`networkInvoke`, `networkFetch`) do.
 */

const MAX_IN_FLIGHT = 6;
const MIN_START_SPACING_MS = 110;

interface PacerState {
  inFlight: number;
  /** Earliest timestamp (Date.now ms) the next call may start. */
  nextStartAt: number;
  waiters: Array<() => void>;
  /** The single armed wake-up timer, so waiters never stack timers. */
  timer: ReturnType<typeof setTimeout> | null;
}

const pacers = new Map<string, PacerState>();

function pacerFor(environmentId: string): PacerState {
  const existing = pacers.get(environmentId);
  if (existing !== undefined) {
    return existing;
  }
  const created: PacerState = {
    inFlight: 0,
    nextStartAt: 0,
    waiters: [],
    timer: null,
  };
  pacers.set(environmentId, created);
  return created;
}

function pump(state: PacerState): void {
  const now = Date.now();
  while (state.waiters.length > 0 && state.inFlight < MAX_IN_FLIGHT) {
    if (now < state.nextStartAt) {
      if (state.timer === null) {
        state.timer = setTimeout(() => {
          state.timer = null;
          pump(state);
        }, state.nextStartAt - now);
      }
      return;
    }
    state.nextStartAt = now + MIN_START_SPACING_MS;
    state.inFlight += 1;
    const waiter = state.waiters.shift();
    waiter?.();
  }
}

function release(state: PacerState): void {
  state.inFlight -= 1;
  pump(state);
}

/**
 * Runs `call` once a concurrency slot and a start-spacing window are available
 * for `environmentId`. Slots are released on settle, success or failure alike.
 */
export async function paceRemoteCall<T>(
  environmentId: string,
  call: () => Promise<T>
): Promise<T> {
  const state = pacerFor(environmentId);
  await new Promise<void>((resolve) => {
    state.waiters.push(resolve);
    pump(state);
  });
  try {
    return await call();
  } finally {
    release(state);
  }
}

/** Test-only: drop all pacer state so suites cannot leak budgets into each other. */
export function resetRequestPacingForTest(): void {
  for (const state of pacers.values()) {
    if (state.timer !== null) {
      clearTimeout(state.timer);
    }
  }
  pacers.clear();
}
