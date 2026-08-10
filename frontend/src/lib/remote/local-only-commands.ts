/**
 * The explicit registry of Tauri commands that must NEVER travel to a remote host,
 * with the reason each one is pinned here (P-11 client half).
 *
 * Two dispositions, because "local-only" covers two genuinely different things:
 *
 * - `run-locally` — the command is owned by THIS client. Its subject is the local
 *   registry, the local transport, or the host facade this Mac itself exposes.
 *   Routing it remotely would either recurse (`remote_invoke` proxying itself) or
 *   mutate the wrong machine's state. It keeps running against local IPC even while
 *   a remote environment is active.
 * - `reject` — the command acts on the LOCAL machine's shell/filesystem using an
 *   identifier that only means something on the host. Running it locally under a
 *   remote environment would silently open the wrong thing, so the wrapper fails it
 *   with a typed `REMOTE_COMMAND_UNAVAILABLE` instead.
 *
 * Classification comes from two places, in this order:
 *
 * 1. The explicit `LOCAL_ONLY_COMMANDS` table below — one reason-coded row per name.
 * 2. The `plugin:` PREFIX RULE (`PLUGIN_COMMAND_PREFIX`) — a whole namespace decided
 *    once instead of 77 per-import fixes. See its own comment block.
 *
 * Absence from both is not an error: an unlisted command is sent to the host,
 * and an unregistered one comes back as `REMOTE_COMMAND_UNAVAILABLE` from the host
 * facade itself (`remote_server/registry.rs` — "unreachable remotely by
 * construction"). The P-11 drift scan is what surfaces unclassified names at CI
 * time; PR 3.1 drives that count to zero.
 */

export type LocalOnlyDisposition = "run-locally" | "reject";

export interface LocalOnlyCommand {
  readonly command: string;
  readonly disposition: LocalOnlyDisposition;
  readonly reason: string;
}

export const LOCAL_ONLY_COMMANDS: readonly LocalOnlyCommand[] = [
  // --- The remote transport itself. Routing any of these remotely recurses. ---
  {
    command: "remote_invoke",
    disposition: "run-locally",
    reason:
      "The proxy command NetworkInvoke dispatches to; routing it remotely is infinite recursion.",
  },
  {
    command: "remote_fetch",
    disposition: "run-locally",
    reason:
      "The proxy command backendFetch dispatches to; routing it remotely is infinite recursion.",
  },
  {
    command: "remote_connect",
    disposition: "run-locally",
    reason: "Opens this client's outbound connection to a host (§6.5).",
  },
  {
    command: "remote_disconnect",
    disposition: "run-locally",
    reason: "Closes this client's outbound connection to a host (§6.5).",
  },
  {
    command: "remote_stream_send",
    disposition: "run-locally",
    reason:
      "Writes a client control frame to THIS client's proxy socket; routing it remotely recurses.",
  },

  // --- This client's environment registry (§6.1/§6.4). ---
  {
    command: "preview_remote_environment",
    disposition: "run-locally",
    reason:
      "Probes a prospective host's descriptor from THIS client before pairing; a remote host cannot answer for a host this client has not paired with yet.",
  },
  {
    command: "pair_remote_environment",
    disposition: "run-locally",
    reason:
      "Pairing writes this client's registry row and Keychain secret; the host has no such registry.",
  },
  {
    command: "list_remote_environments",
    disposition: "run-locally",
    reason: "Reads this client's registry, which is not host state.",
  },
  {
    command: "remove_remote_environment",
    disposition: "run-locally",
    reason:
      "Staged removal mutates this client's row + Keychain (P-27); a host cannot perform it.",
  },
  {
    command: "get_active_environment",
    disposition: "run-locally",
    reason:
      "Reads the Rust-side active-environment mirror that authorizes the proxy (P-26).",
  },
  {
    command: "set_active_environment",
    disposition: "run-locally",
    reason:
      "Writes the Rust-side active-environment mirror; routing it remotely would ask a host to switch the client.",
  },

  // --- This Mac's own host facade (§5.4). Never exposed remotely by design. ---
  {
    command: "start_remote_listener",
    disposition: "run-locally",
    reason: "Controls the listener THIS Mac exposes, not the peer's.",
  },
  {
    command: "stop_remote_listener",
    disposition: "run-locally",
    reason: "Controls the listener THIS Mac exposes, not the peer's.",
  },
  {
    command: "set_remote_exposure_mode",
    disposition: "run-locally",
    reason: "Controls the listener THIS Mac exposes, not the peer's.",
  },
  {
    command: "get_remote_listener_status",
    disposition: "run-locally",
    reason: "Reports the listener THIS Mac exposes, not the peer's.",
  },
  {
    command: "list_remote_advertised_endpoints",
    disposition: "run-locally",
    reason:
      "The URLs THIS Mac advertises for its own listener; a peer's endpoints are not pairable from here.",
  },

  // --- Host-side device/pairing/session admin (§3.1: no remote admin surface). ---
  {
    command: "generate_remote_pairing_code",
    disposition: "run-locally",
    reason:
      "Device/pairing management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "list_remote_pairing_codes",
    disposition: "run-locally",
    reason:
      "Device/pairing management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "revoke_remote_pairing_code",
    disposition: "run-locally",
    reason:
      "Device/pairing management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "list_remote_devices",
    disposition: "run-locally",
    reason:
      "Device/pairing management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "set_remote_device_agent_control",
    disposition: "run-locally",
    reason:
      "The per-device ui:agent grant is a host-owner decision made at the host UI (§5.4).",
  },
  {
    command: "revoke_remote_device",
    disposition: "run-locally",
    reason:
      "Device/pairing management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "list_remote_sessions",
    disposition: "run-locally",
    reason:
      "Session management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "disconnect_remote_session",
    disposition: "run-locally",
    reason:
      "Session management is host-UI-only via loopback commands (§3.1, §5.4).",
  },
  {
    command: "list_remote_audit_entries",
    disposition: "run-locally",
    reason:
      "Reads THIS Mac's own remote-access audit log; the pane's other rows are all local (§3.1, §5.4).",
  },

  // --- Local app chrome whose subject is this Mac. ---
  {
    command: "open_startup_logs",
    disposition: "run-locally",
    reason:
      "Opens THIS client's own launch log; the user asking for it is sitting at this Mac.",
  },

  // --- This Mac's dock icon. ---
  //
  // The COUNT is the host's (the notification reads are all registered on the facade and
  // should follow the active environment), but painting the badge is a local act:
  // `set_dock_badge_count` runs `set_macos_dock_badge` on the running process's main thread,
  // so routing it remotely badges the HOST's dock with this client's number and leaves this
  // Mac's icon stale. `run-locally` rather than `reject` — the badge must keep working while
  // a remote environment is active, showing the host's count on this Mac's dock.
  {
    command: "set_dock_badge_count",
    disposition: "run-locally",
    reason:
      "Paints THIS Mac's dock icon on its own main thread; the host has its own dock and its own badge. The count it displays is still the active environment's.",
  },

  // --- This client's own boot lifecycle. The mount gate cannot be answered by a host. ---
  //
  // `StartupRoot` scopes its QueryClient to LOCAL_ENVIRONMENT_ID, but cache scoping and
  // transport routing are different axes: `invoke` routes on the GLOBAL active
  // environment, so after a reload with a remote environment active these went to the
  // host, where they are unregistered. The shell-paint handoff then rejected and the
  // client never mounted the app at all.
  {
    command: "get_startup_status",
    disposition: "run-locally",
    reason:
      "The mount gate's own snapshot: bootId, attemptId, and readiness of THIS process. A host's boot state cannot authorize this client to mount.",
  },
  {
    command: "get_startup_diagnostics",
    disposition: "run-locally",
    reason:
      "Diagnostics for THIS client's launch, read from the local startup coordinator.",
  },
  {
    command: "retry_startup",
    disposition: "run-locally",
    reason:
      "Retries THIS client's own failed startup attempt; a host has no authority over this process's boot.",
  },
  {
    command: "report_startup_frontend_milestone",
    disposition: "run-locally",
    reason:
      "Reports this client's shell-paint milestone against a local bootId/attemptId pair the host has never heard of; the handoff that mounts the app depends on it succeeding.",
  },

  // --- Host-path shell launches: wrong machine if run locally. ---
  {
    command: "open_agent_conversation_workspace",
    disposition: "reject",
    reason:
      "Launches an editor at a workspace path that exists on the host filesystem, not this Mac's.",
  },
  {
    command: "open_agent_conversation_workspace_path",
    disposition: "reject",
    reason:
      "Launches a file manager at a workspace path that exists on the host filesystem, not this Mac's.",
  },
  {
    command: "open_agent_terminal",
    disposition: "reject",
    reason:
      "PTY control over a host-side terminal; terminal is excluded from remote v1 (§3.3).",
  },
] as const;

// ---------------------------------------------------------------------------
// The `plugin:` prefix rule (Phase 2 — close the Tauri-plugin side door).
// ---------------------------------------------------------------------------
//
// `frontend/vite.config.ts` aliases `@tauri-apps/api/core` for the WHOLE module graph,
// node_modules included, so every `@tauri-apps/plugin-*` package invokes through this
// wrapper too — 77 import sites across seven plugins (opener, dialog, fs, updater,
// process, global-shortcut, notification). None of their `plugin:<name>|<command>` names
// is in `remote_server/registry.rs` (the facade is exhaustive over `generate_handler!`,
// which plugin commands bypass by construction), so before this rule every one of them
// travelled to the host and answered `REMOTE_COMMAND_UNAVAILABLE`.
//
// That was not merely unavailable, it was pointed at the wrong machine. Each of these
// plugins acts on the DEVICE THE UI IS RUNNING ON: `plugin:opener|open_url` opens this
// user's browser, `plugin:updater|check` asks about THIS app binary's update, global
// shortcuts bind THIS keyboard, `plugin:notification|is_permission_granted` reads THIS
// macOS notification-permission grant, and the dialog/fs pickers address THIS
// filesystem. Sending them to the host either did nothing visible or acted on the host
// operator's machine.
//
// So the disposition for the namespace is `run-locally`, and it is expressed ONCE as a
// prefix rather than as 77 call-site edits or 51 near-identical table rows: a plugin the
// app adds tomorrow inherits the correct routing instead of silently reopening the door.
//
// `run-locally` and not `reject` for the same reason as the dock badge above — these
// affordances must keep WORKING while a remote environment is active, on this device.
// The host-path cases are already handled at a different seam: `openPath` /
// `revealItemInDir` on host-side workspace paths are suppressed by host-affordance
// gating (`host-affordances.ts`) and degrade to `HostPathCopyButton`, so the prefix rule
// never opens a host path locally.
export const PLUGIN_COMMAND_PREFIX = "plugin:";

/** The one reason every `plugin:*` name inherits, so the prefix stays reason-coded. */
export const PLUGIN_PREFIX_REASON =
  "Tauri plugin command: opener/dialog/fs/updater/process/global-shortcut/notification " +
  "all act on THIS device — the browser, keyboard, filesystem, app binary and macOS " +
  "permission grants of the machine showing the UI. The host has its own. Classified by " +
  "the `plugin:` prefix rule, not by a per-command row.";

/**
 * Reviewed exceptions: `plugin:*` names that must target the HOST instead.
 *
 * DELIBERATELY EMPTY. The Phase 2 review swept all seven plugins and all 77 import sites
 * and found no plugin call whose subject is the host. The list exists so that a future
 * exception is a reviewed, named decision rather than a silent hole in the prefix rule —
 * and so it costs something: an excepted name leaves local-only classification entirely
 * and must then earn a facade registration or a ledger disposition, or the P-11 drift
 * scan reports it unclassified and CI goes red.
 */
export const HOST_TARGETED_PLUGIN_COMMANDS: readonly string[] = [];

const HOST_TARGETED_PLUGIN_COMMAND_SET: ReadonlySet<string> = new Set(
  HOST_TARGETED_PLUGIN_COMMANDS
);

/** `plugin:opener|open_url` → true. The namespace the prefix rule governs. */
export function isPluginCommand(cmd: string): boolean {
  return cmd.startsWith(PLUGIN_COMMAND_PREFIX);
}

const LOCAL_ONLY_BY_COMMAND: ReadonlyMap<string, LocalOnlyCommand> = new Map(
  LOCAL_ONLY_COMMANDS.map((entry) => [entry.command, entry])
);

export function findLocalOnlyCommand(cmd: string): LocalOnlyCommand | undefined {
  const explicit = LOCAL_ONLY_BY_COMMAND.get(cmd);
  if (explicit !== undefined) return explicit;

  if (isPluginCommand(cmd) && !HOST_TARGETED_PLUGIN_COMMAND_SET.has(cmd)) {
    return {
      command: cmd,
      disposition: "run-locally",
      reason: PLUGIN_PREFIX_REASON,
    };
  }

  return undefined;
}

export function isLocalOnlyCommand(cmd: string): boolean {
  return findLocalOnlyCommand(cmd) !== undefined;
}

/**
 * Every EXPLICITLY pinned command name, for the P-11 drift scan.
 *
 * Not exhaustive over local-only routing any more: the `plugin:` namespace is classified
 * by prefix, so ask `isLocalOnlyCommand`/`findLocalOnlyCommand` rather than this list.
 */
export const LOCAL_ONLY_COMMAND_NAMES: readonly string[] = LOCAL_ONLY_COMMANDS.map(
  (entry) => entry.command
);
