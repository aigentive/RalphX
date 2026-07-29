# Remote Access

Use RalphX on one machine to watch and steer work running on another.

Your Mac keeps doing the work — running agents, holding your repositories, spawning processes.
A second RalphX (or, later, the mobile app) connects to it over your network and gives you a
window into it. Nothing moves off the host.

> **Status:** Remote Access ships dark. The host side is behind the `remote_host` setting and the
> client side behind the `remoteEnvironments` flag. Both default to off.

---

## The two modes

RalphX does not have a "server build" and a "client build". Any RalphX can be either, or both.

**Host mode** — this machine accepts connections. It opens a second, authenticated listener
(default port `3849`) alongside the local backend. Your agents, worktrees, and settings stay
exactly where they are.

**Client mode** — this machine connects to one or more hosts. Each host you pair with becomes an
*environment* you can switch between, alongside your own Local environment.

Environments are fully isolated from each other. Switching environments does not disturb the one
you left, and a remote host going offline never affects your Local environment.

---

## Turning on host mode

1. **Settings → Remote Access → Enable host mode.**
2. **Choose how it is exposed.**
   - *Loopback* — only this machine can reach it. Useful for testing.
   - *Tailnet* — reachable from your Tailscale network.

   There is no "expose to the whole internet" option, and the listener will refuse to bind a
   wildcard address. If you want access from outside your tailnet, put it behind something that
   terminates TLS and authenticates — do not port-forward to `3849`.
3. The pane shows the listener's status, the address clients should use, and every paired device.

---

## Pairing a device

Pairing is deliberately a face-to-face gesture: you must be able to see the host's screen.

1. On the **host**: Settings → Remote Access → **Add device**. A short pairing code appears, with
   a copyable URL and a QR code.
2. On the **client**: Settings → Environments → **Add environment**, then enter the code (or scan
   the QR).
3. The host mints a long-term token for that device and lists it by name.

Things worth knowing:

- **A pairing code works exactly once**, and it expires on its own. If you fumble it, generate a
  new one — a used code cannot be reused, even by you.
- **The token never appears in the RalphX interface** after pairing. It is stored in the client's
  system Keychain and is not readable from the app.
- Each device gets its own token. Revoking one does not touch the others.

---

## What you are actually granting

This is the part worth reading slowly.

### The default: a viewer with brakes

A freshly paired device gets `ui:read` and `ui:operate`. In plain terms, it can:

- **See everything** — tasks, plans, conversations, diffs, agent output, notifications.
- **Stop everything safely** — deny a permission request, or use the global pause/stop controls.
- **Make small edits** — change a task's category or priority, create tasks in the Backlog.
- **Handle attachments** — upload and retrieve device-scoped task attachments.

That is a **viewer with brakes**. It can see your work and it can halt your work. It cannot
*start* work.

The split is intentional. Stopping something is safe: the worst case is that an agent idles.
Starting something is not. So the brakes are handed out by default and the accelerator is not.

Per-task block, pause, and stop controls are part of the agent-control grant, as is bulk group
cancel. They are not pure brakes in the execution engine: leaving an agent-active task can run
execution-exit Git work, and `block_task` frees capacity and asks the scheduler to start queued
work. The safe default-tier brakes are the **global** pause and stop controls, which set the
process-wide pause gate before transitioning any task, so no replacement agent can launch.

### The upgrade: "Allow remote agent control"

`ui:agent` is a separate, **off-by-default, per-device** toggle on the host. Turning it on lets
that device start agent runs, send chat messages that steer an agent, resume runs, and write the
kinds of records a background loop turns into a spawn. It also enables per-task block/pause/stop
and bulk group cancellation because those operations can trigger agent-active exit behavior.

**Anyone who steals a `ui:agent` token can run arbitrary code on your Mac.**

That is not a worst-case reading of the grant. It is what the grant *is*. An agent run executes
commands, writes files, and installs things in your working directory, under your user account,
with your credentials on disk. Handing a device `ui:agent` is handing it the ability to execute
code on the host machine. Treat that token exactly as you would treat SSH access.

Concretely, before you enable it:

- Grant it per device, never as a habit. Enable it on the phone you actually carry; leave it off
  on the laptop in the drawer.
- Only over a network you control. A tailnet is a reasonable trust boundary. A café's Wi-Fi with
  a forwarded port is not.
- Revoke it the moment a device is lost, sold, or handed to someone else.
- If you would not give the device an SSH key to this machine, do not give it `ui:agent`.

There is no configuration that softens this. The grant is powerful because agent control is
powerful.

### What no device can do

Some things are refused regardless of grant, because there is no safe version of them over a
network:

- **Terminal / shell access.** No PTY is exposed, no terminal command is reachable, and the
  terminal drawer is hidden for remote environments. Not "restricted" — absent.
- **Credential and authentication setup.** Git and GitHub auth configuration is not reachable.
- **Arbitrary path writes** and process spawns outside the agent surface.

These are refused by construction: the commands are not on the remote allowlist at all, and the
build fails if someone tries to add one under an insufficient permission level.

---

## Managing and revoking devices

The host's device list shows each device's name, the client version it reported, and when it was
last seen. Each row has a **Revoke** action, and each has its own agent-control toggle.

**Revocation is immediate and applies to live sessions.** The device's open connection is closed
within the heartbeat window — it does not keep streaming until it happens to reconnect — and its
next request is refused. Revoking is durable-first, so it survives a host restart even if the
client never hears about it.

Removing an environment on the client side asks the host to revoke first, then deletes the local
token and the environment row. If the host is unreachable, the local removal still completes;
revoke the device from the host when you next can.

---

## Living with a remote connection

**Going offline.** If the host stops answering, the client shows a disconnected state and keeps
retrying with a backoff. Dead hosts are detected within about a minute. Your Local environment is
untouched.

**Coming back.** On reconnect, the client resumes from where it left off when it safely can, and
otherwise re-fetches from scratch. You may briefly see a loading state — that is the client
choosing correctness over a fast-looking-but-wrong transcript.

**Missed events.** RalphX keeps a bounded event history. If you are away long enough for it to
age out, the client refetches rather than showing you a transcript with an invisible hole.

**Things that never replay.** Live typing (streaming agent output) and permission prompts are not
stored for replay. They are recovered by re-reading the actual message and re-asking for the
pending prompt list, so a prompt raised while you were disconnected still appears when you
return — it is never silently lost.

**Background environments.** Environments you are not looking at can still surface a notification
count, but they do not sync in the background. Switching to one always loads it fresh.

---

## What stays on the host

Everything that matters:

- Your repositories, worktrees, and every file an agent touches.
- Agent processes and their output.
- Credentials — git, GitHub, provider API keys. None are readable over the remote surface.
- The database, the event history, and all settings.

The client holds one thing: the device token for each environment it has paired, in the system
Keychain.

---

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| "Cannot reach host" | Host mode off, machine asleep, or not on the same tailnet |
| Pairing code rejected | Already used, or expired — generate a new one |
| An action is greyed out | The device lacks the scope; check its toggles on the host |
| "Update required" | Client older than the host's supported floor |
| Stuck reconnecting | Check the host's Remote Access pane for listener errors |
| Terminal missing | Expected — terminal is not available remotely |

---

## For developers

The wire protocol, error taxonomy, capability classes, and event-stream semantics are documented
in [`docs/architecture/remote-protocol.md`](../architecture/remote-protocol.md).
