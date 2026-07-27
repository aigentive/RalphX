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
 * Absence from this file is not an error: an unlisted command is sent to the host,
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

  // --- This client's environment registry (§6.1/§6.4). ---
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

  // --- Local app chrome whose subject is this Mac. ---
  {
    command: "open_startup_logs",
    disposition: "run-locally",
    reason:
      "Opens THIS client's own launch log; the user asking for it is sitting at this Mac.",
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

const LOCAL_ONLY_BY_COMMAND: ReadonlyMap<string, LocalOnlyCommand> = new Map(
  LOCAL_ONLY_COMMANDS.map((entry) => [entry.command, entry])
);

export function findLocalOnlyCommand(cmd: string): LocalOnlyCommand | undefined {
  return LOCAL_ONLY_BY_COMMAND.get(cmd);
}

export function isLocalOnlyCommand(cmd: string): boolean {
  return LOCAL_ONLY_BY_COMMAND.has(cmd);
}

/** Every command name pinned here, for the P-11 drift scan. */
export const LOCAL_ONLY_COMMAND_NAMES: readonly string[] = LOCAL_ONLY_COMMANDS.map(
  (entry) => entry.command
);
