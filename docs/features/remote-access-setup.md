# Remote Access — End-to-End Setup Guide

Step-by-step for getting one RalphX (the **host**) reachable from another (the **client**),
including the Tailscale account setup both sides need.

For what Remote Access *is* and what a paired device is allowed to do, read
[`remote-access.md`](./remote-access.md) first — this guide is the mechanical walkthrough.

---

## What you end up with

Your work Mac keeps running agents, holding repositories, and spawning processes. A second
machine connects to it over your private tailnet and gives you a window into it. Nothing moves
off the host, and nothing is exposed to the public internet.

## Before you start

| Requirement | Why |
|---|---|
| Two machines, both running RalphX | There is no separate "server build" — any RalphX can be host, client, or both |
| A Tailscale account | Remote Access is **tailnet-only**. There is no LAN or public mode. |
| Tailscale installed on **both** machines | The host binds a tailnet address; the client dials one |
| Physical access to the host's screen at pairing time | Pairing is deliberately face-to-face — you read a code off the host |

**Time:** ~15 minutes, most of it Tailscale.

---

## Part 1 — Tailscale

### The short version

**Both machines need Tailscale installed, running, and signed into the same account.** Tailscale
is the network underneath — without it on both ends there is no route between them, whichever
machine is hosting.

On **each** machine, once:

```bash
brew install tailscale                 # CLI + daemon
sudo brew services start tailscale     # run it now, and at every boot
tailscale up                           # prints an auth URL — open it and sign in
tailscale status                       # confirm: you should see a 100.x.y.z address
```

That is the whole setup. You authenticate **once per machine** — the node key is stored, so it
survives reboots and RalphX restarts. You do not log in again each session.

Then, on the host only, disable key expiry (see §1.5) so it does not silently drop off the
tailnet in six months.

### Who needs what

| | Host (serves) | Client (connects) |
|---|---|---|
| Tailscale installed + daemon running | Yes | Yes |
| Signed into the same tailnet | Yes | Yes |
| RalphX invokes the `tailscale` CLI | **Yes** — `status`, and `serve` in Serve mode, so the binary must be resolvable by RalphX (§1.2.2) | No — it only makes HTTP requests to the host's tailnet address |
| MagicDNS + HTTPS Certificates | Only for Serve mode (§1.4) | No |
| Key expiry disabled | Recommended (§1.5) | Optional |

The rest of Part 1 is the detail behind those four commands. Skim it if the short version worked.

### 1.1 Create the tailnet

1. Go to [tailscale.com](https://tailscale.com) and sign up. The free Personal plan is enough.
   You authenticate with an existing identity provider (Google, GitHub, Microsoft, Apple…) — the
   account you pick becomes the owner of your tailnet.
2. You now have a *tailnet*: a private network only your devices can join.

### 1.2 Install on the host

Pick **one** of these. They are not equivalent — see the CLI note below.

| Install method | What you get | CLI on `PATH`? |
|---|---|---|
| `brew install tailscale` (formula) | CLI + `tailscaled` daemon, no GUI | **Yes** — `/opt/homebrew/bin/tailscale` |
| `brew install --cask tailscale` | The macOS GUI app | No — see §1.2.2 |
| [tailscale.com/download](https://tailscale.com/download) | The macOS GUI app | No — see §1.2.2 |

### 1.2.1 Start the daemon (Homebrew formula only)

The formula installs the CLI **and** the `tailscaled` daemon, but does not start it. The GUI
variants run their daemon for you; the formula does not. Until you start it, every CLI command
fails with `failed to connect to local Tailscale service; is Tailscale running?`:

```bash
sudo brew services start tailscale     # installs a LaunchDaemon; root is required for the TUN interface
brew services list | grep tailscale    # should now read "started"
```

Skip this if you installed a GUI variant.

### 1.2.2 macOS GUI variants: making the CLI reachable

**Skip this entire section if you used `brew install tailscale`** — the formula puts `tailscale`
at `/opt/homebrew/bin/tailscale`, which is already on your `PATH` *and* is one of the fixed paths
RalphX checks. Nothing to do.

This applies only to the **cask / direct-download GUI app**, where the app bundle *is* the CLI —
the same binary switches to CLI mode when run from a terminal — and is not on your `PATH`. Add it:

```bash
# ~/.zshrc
export PATH="/Applications/Tailscale.app/Contents/MacOS:$PATH"
```

> **Use a `PATH` export, not a shell alias.** A common suggestion is
> `alias tailscale="/Applications/Tailscale.app/Contents/MacOS/Tailscale"`. That works when *you*
> type it, but **RalphX will not find it** — aliases exist only inside an interactive shell and
> are invisible to a spawned process. RalphX resolves the binary by looking on `PATH`, then at
> fixed locations (`/Applications/Tailscale.app/Contents/MacOS/tailscale`,
> `/opt/homebrew/bin/tailscale`, `/usr/local/bin/tailscale`), then via a login-shell
> `command -v`. A `PATH` export in your shell profile is found by that last step; an alias never is.

Verify RalphX's view, not just your shell's:

```bash
zsh -lic 'command -v tailscale'   # this is roughly what RalphX does
```

If that prints a path, RalphX will find it. If it prints nothing, Serve mode will report
`cliUnavailable` (§2.3).

### 1.2.3 Log in from the CLI

Signing in through the GUI works, but the CLI is faster and is the only option on a headless or
`tailscaled`-only host.

```bash
tailscale up
```

This prints an authentication URL. Open it, sign in, and the machine joins your tailnet. Useful
variants:

```bash
tailscale up --hostname=work-mac       # name the device explicitly
tailscale login --qr                   # render the login URL as a QR code
tailscale login                        # re-authenticate / switch accounts
```

For an unattended or scripted host, mint an auth key in the admin console
(**Settings → Keys**) and skip the browser entirely:

```bash
tailscale up --auth-key=tskey-auth-xxxxxxxxxxxx
```

Treat an auth key like a password — anyone holding it can add a device to your tailnet.

### 1.2.4 Confirm it worked

```bash
tailscale status      # lists your devices; yours should show a 100.x.y.z address
tailscale ip -4       # this machine's tailnet IPv4 — the address clients dial in Tailnet direct
```

The machine should also now appear in your
[admin console](https://login.tailscale.com/admin/machines).

To disconnect later: `tailscale logout` (expires the auth; the next `tailscale up` re-prompts).

### 1.3 Install on the client

Same four commands, **same Tailscale account**. Two devices on different tailnets cannot see each
other, and the failure looks like a RalphX problem rather than a network one.

The client is simpler than the host: RalphX never shells out to the `tailscale` CLI on this side,
it just makes HTTP requests to the host's tailnet address. So the daemon has to be running and
signed in, but you do not need the binary to be resolvable *by RalphX* the way §1.2.2 describes —
that requirement is host-only.

Verify from the client that it can reach the host:

```bash
tailscale status          # host should be listed
ping <host-tailnet-ip>    # e.g. ping 100.101.102.103
```

### 1.4 Enable MagicDNS + HTTPS — only if you want *Tailscale Serve*

RalphX offers two exposure modes. **Tailscale Serve** gives you TLS terminated at the tailnet
edge and a friendly hostname; it requires two tailnet-level features that are **off by default**:

1. Admin console → **DNS** → enable **MagicDNS**.
2. Same page → enable **HTTPS Certificates**.

Skip this section if you plan to use *Tailnet direct* — that mode needs neither.

> RalphX runs `tailscale serve --bg --https=443 http://127.0.0.1:<port>` on your behalf when you
> pick Serve mode, and releases it with `tailscale serve --https=443 off` when you turn host mode
> off. Without HTTPS Certificates enabled, that command fails and RalphX reports the listener as
> degraded.

### 1.5 Keep it running — disable key expiry on the host

Tailscale node keys **expire after 180 days by default** (configurable 1–180). When a key expires
the device drops off the tailnet and "connections to/from the given endpoint stop working" — which
for a RalphX host means every paired client silently loses it until someone re-authenticates at
the host's keyboard.

For a machine that exists to be connected to, turn that off:

**[Admin console](https://login.tailscale.com/admin/machines) → Machines → your host's ⋯ menu →
Disable key expiry.**

There is no CLI equivalent — it is admin-console only. Tailscale explicitly endorses this for
"trusted servers, subnet routers, or remote IoT devices that are hard to reach": you trade
periodic reauthentication for the device staying reachable. A RalphX host is exactly that case.

The client matters less — if its key expires you are sitting in front of it and can just run
`tailscale up` again — but there is no harm in disabling it there too.

> This is separate from RalphX's own pairing tokens, which never expire on a schedule and are
> revoked per-device from the Remote Access pane. Tailscale controls whether the machines can
> reach each other; RalphX controls whether a paired device is allowed to do anything.

---

## Part 2 — Host setup

### 2.1 Make the panes visible

The Remote Access and Connections panes are behind a client-owned feature flag.

`config/ralphx.yaml` ships with it enabled:

```yaml
ui:
  feature_flags:
    remote_environments: true
```

Or override per launch without editing the file:

```bash
RALPHX_UI_REMOTE_ENVIRONMENTS=true npm run tauri dev
```

**Restart RalphX after changing this.** The config is read once per process (a `OnceLock`) and
the frontend fetches the flags once at boot — a window reload is not enough.

### 2.2 Enable host mode

**Settings → Integrations → Remote Access → Enable remote access.**

This is a separate switch from the feature flag, persisted in the database and defaulting to
**off**. The flag reveals the pane; this toggle starts the listener.

### 2.3 Choose the exposure mode

Both modes are tailnet-only and both carry all traffic inside WireGuard. The listener **refuses
to bind** a wildcard or LAN address — the bind address must sit inside `100.64.0.0/10`.

> Do not port-forward `3849` to the internet. If you need access from outside your tailnet, put
> it behind something that terminates TLS and authenticates.

#### Which one should I pick?

**Start with Tailnet direct.** It has fewer moving parts and no tailnet-level prerequisites, so
if pairing fails you know the problem is RalphX and not Tailscale. Move to Serve once it works
and you want the nicer hostname or a real TLS certificate.

| | **Tailnet direct** | **Tailscale Serve** |
|---|---|---|
| **Pick it when** | Getting started; debugging; two machines you control | You want a stable hostname, a real cert, or a browser/mobile client that insists on HTTPS |
| **Tailnet setup needed** | None | MagicDNS **and** HTTPS Certificates (§1.4) |
| **Client-facing address** | `http://100.x.y.z:3849` | `https://<machine>.<tailnet>.ts.net` (port 443) |
| **What RalphX binds** | The tailnet IP, port `3849` | Loopback `127.0.0.1:<port>` only |
| **Reachable on the tailnet by** | Anything on your tailnet that can route to `:3849` | The Serve proxy |
| **TLS** | None — plaintext HTTP inside the WireGuard tunnel | TLS terminated at the tailnet edge, on top of WireGuard |
| **Network hops** | Client → host listener | Client → `tailscaled` proxy → host listener (one extra hop) |
| **Moving parts at startup** | Bind a socket | Bind a socket, resolve the `tailscale` CLI, acquire a Serve mapping, provision/renew a cert |
| **External process invoked** | None | `tailscale serve --bg --https=443 http://127.0.0.1:<port>` |
| **Teardown obligation** | Close the socket | Release the mapping (`tailscale serve --https=443 off`) — RalphX does this on disable |
| **Ways it can fail** | Address not in `100.64.0.0/10`; port in use | The four below, plus everything Tailnet direct can hit |

#### Reading the fine print

**"TLS: none" is not the same as unencrypted.** Both modes ride inside WireGuard, so nothing is
in the clear on the wire either way. Serve adds a *second* layer plus a certificate a browser
will accept. That matters for defense in depth and for clients that refuse plain HTTP — not for
whether a passive observer can read your traffic.

**Bind surface is the sharper security difference.** In Serve mode RalphX binds loopback only, so
the listener is not directly addressable from the tailnet at all — every request arrives through
the Serve proxy. In Tailnet direct, `:3849` is reachable by anything on your tailnet that can
route to it. Both are still behind RalphX's own bearer-token auth, so this is a matter of layers,
not of one being open.

**No measured performance comparison exists.** Serve adds one proxy hop and TLS termination, so
it cannot be faster — but nobody has benchmarked the difference, and neither mode has been shown
to be a bottleneck. Do not pick on performance grounds; pick on setup cost and whether you need a
certificate.

#### When Serve fails, it fails in one of four ways

RalphX reports a typed reason rather than prose, so the pane can tell you what to actually do:

| Kind | Meaning | Fix |
|---|---|---|
| `cliUnavailable` | The `tailscale` binary could not be resolved | Install Tailscale, or put it on `PATH` (§1.2.2). If `tailscale` works in your terminal but RalphX still reports this, you almost certainly have a shell *alias* rather than a `PATH` entry — check with `zsh -lic 'command -v tailscale'` |
| `launchFailed` | The binary exists but would not start | Check permissions and that the Tailscale app is running |
| `timeout` | The command hung | Check `tailscale status`; the daemon may be wedged |
| `commandFailed` | The command ran and was refused | Usually MagicDNS/HTTPS Certificates not enabled, or not logged in (§1.4) |

If acquiring the Serve mapping fails, RalphX **releases any mapping an earlier run left behind**
rather than staying silently tailnet-reachable. A degraded Serve listener is not a half-open one.

### 2.4 Generate a pairing code

**Remote Access → *Pair a device* → Generate pairing code.**

You get a short code, a copyable URL, and a QR code. Keep this screen up — you need it on the
client in the next step.

- A pairing code works **exactly once** and expires on its own.
- A used code cannot be reused, even by you — generate a new one if you fumble it.
- After six failed attempts, pairing is rate-limited.

---

## Part 3 — Client setup

### 3.1 Make the panes visible

Same as §2.1 — the flag is **per device**, so enabling it on the host does not enable it on the
client. Set it and restart.

### 3.2 Add the environment

**Settings → Integrations → Connections → Add environment**, then enter the pairing code (or
scan the QR).

On success the host mints a long-term token for this device and lists it by name.

- The token is stored in the client's system Keychain and is **never shown in the interface**
  after pairing.
- Each device gets its own token; revoking one does not touch the others.

### 3.3 Switch to it

The environment switcher in the top bar now lists your Local environment plus the host you
paired with. Switching does not disturb the environment you left, and a host going offline never
affects Local.

---

## Part 4 — Granting agent control (optional)

A freshly paired device is a **viewer with brakes**: it can see everything and stop everything,
but it cannot start work.

To let it steer agents, go to the host's **Remote Access → device list** and enable agent control
for that specific device. A confirmation dialog spells out what you are granting — read it. It
covers code execution on the host machine, and withdrawal disconnects live sessions immediately.

This is per-device and reversible at any time.

---

## Verifying it works

| Check | Where | Expected |
|---|---|---|
| Host listener is up | Host → Remote Access pane | Status shows running, with the address clients should use |
| Device is paired | Host → device list | Client listed by name, with paired date and last-seen |
| Client is connected | Client → environment switcher | Host environment present and reporting connected |
| Traffic is tailnet-only | `tailscale status` on either machine | The peer connection is listed |

---

## Turning it off

| Goal | Action |
|---|---|
| Disconnect one device | Host → Remote Access → device list → revoke. Its next request is refused and its live sessions are killed. |
| Stop serving entirely | Host → Remote Access → disable. Releases the Tailscale Serve mapping if one was acquired. |
| Hide the panes again | Set `remote_environments: false` and restart |

---

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Remote Access / Connections panes missing | Flag off, or RalphX not restarted after changing it |
| Pane visible, but nothing serves | Host mode toggle (§2.2) is separate from the flag — enable it too |
| Listener degraded in Serve mode | MagicDNS or HTTPS Certificates not enabled (§1.4), or the `tailscale` CLI is not resolvable |
| Bind refused | The address is outside `100.64.0.0/10`. Tailscale may not be connected. |
| Pairing code rejected | Already used, or expired. Generate a new one. |
| Client cannot see the host at all | Different tailnet accounts, or Tailscale disconnected on either end |
| Version mismatch on pairing | Host and client protocol versions must match — update both to the same RalphX build |

---

## Known limitations in this build

Be aware before you invest time:

- **Projects do not load remotely.** The project-listing commands shell out to git and are
  deliberately not exposed on the remote facade, so a remote environment currently lands on the
  no-projects Welcome screen. This is a known gap, not a misconfiguration.
- **Tool-call detail is unavailable remotely.** Transcripts load, but expanding a tool call in one
  fails — there is no spawn-free variant of that read yet.
- **Some Agents surfaces are host-only**, including starting a conversation and the queued-message
  views. Affordances that are unavailable remotely are shown as such rather than failing silently.
- **Remote send is delivered in-band.** A message sent remotely is refused, actionably, when no
  run is live — there is no background queue draining it.
