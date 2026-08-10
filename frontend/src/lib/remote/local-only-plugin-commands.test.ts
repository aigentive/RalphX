/**
 * The `plugin:` prefix rule — Phase 2, "close the Tauri-plugin side door".
 *
 * `vite.config.ts` aliases `@tauri-apps/api/core` for the whole module graph, node_modules
 * included, so every `@tauri-apps/plugin-*` package invokes through the remote wrapper. None
 * of their `plugin:<ns>|<cmd>` names can ever be registered on the host facade (the facade is
 * exhaustive over `generate_handler!`, which plugin commands bypass), and every one of them
 * acts on the DEVICE SHOWING THE UI. So the namespace routes locally, decided once by prefix.
 *
 * These cases are the ratchet: they fail if the rule is removed, if it is narrowed to a
 * hand-listed subset that a newly added plugin would fall out of, or if an exception is added
 * without the review the exception list exists to force.
 */

import { describe, expect, it } from "vitest";

import {
  HOST_TARGETED_PLUGIN_COMMANDS,
  PLUGIN_COMMAND_PREFIX,
  findLocalOnlyCommand,
  isLocalOnlyCommand,
  isPluginCommand,
} from "./local-only-commands";

/**
 * One name per plugin package the app imports, taken from the packages' own `invoke(...)`
 * literals. The user-visible defect each one caused before the rule landed is spelled out,
 * so a future reader can tell whether a proposed exception is re-breaking it.
 */
const PLUGIN_COMMANDS: readonly (readonly [string, string])[] = [
  ["plugin:opener|open_url", "PR/docs/OAuth links opened the HOST operator's browser"],
  ["plugin:opener|open_path", "opened a path on the host's filesystem"],
  ["plugin:opener|reveal_item_in_dir", "revealed a file in the host's Finder"],
  ["plugin:updater|check", "asked the host whether THIS app binary has an update"],
  ["plugin:updater|download_and_install", "would have updated the host's app, not this one"],
  ["plugin:process|restart", "would have relaunched the host app"],
  ["plugin:global-shortcut|register", "bound this client's hotkey on the host's keyboard"],
  ["plugin:global-shortcut|unregister", "left this client's hotkey bound forever"],
  [
    "plugin:notification|is_permission_granted",
    "the probe rejected remotely and short-circuited a registered settings write",
  ],
  ["plugin:dialog|open", "asked the host to show a file picker nobody was sitting at"],
  ["plugin:fs|read_text_file", "read the host's filesystem instead of this device's"],
];

describe("plugin: commands are classified local by one prefix rule", () => {
  it.each(PLUGIN_COMMANDS)("routes %s locally (was: %s)", (cmd) => {
    const entry = findLocalOnlyCommand(cmd);
    expect(entry, `${cmd} must be classified by the plugin: prefix rule`).toBeDefined();
    // `run-locally`, never `reject`: these affordances must keep WORKING on this device
    // while a remote environment is active. `reject` would trade a wrong-machine action
    // for a dead button.
    expect(entry?.disposition).toBe("run-locally");
    expect(entry?.command).toBe(cmd);
    expect(entry?.reason).toBeTruthy();
    expect(isLocalOnlyCommand(cmd)).toBe(true);
  });

  it("classifies by PREFIX, not by an enumerated list a new plugin would fall out of", () => {
    // The whole point of the rule: a plugin added tomorrow inherits the routing. If someone
    // replaces the prefix with 51 table rows, this is the case that notices.
    const notYetInstalled = `${PLUGIN_COMMAND_PREFIX}some-future-plugin|do_thing`;
    expect(isPluginCommand(notYetInstalled)).toBe(true);
    expect(findLocalOnlyCommand(notYetInstalled)?.disposition).toBe("run-locally");
  });

  it("leaves ordinary host commands alone — the rule is scoped to the namespace", () => {
    for (const cmd of ["list_tasks", "update_notification_settings", "open_terminal"]) {
      expect(isPluginCommand(cmd)).toBe(false);
    }
    // A host command must stay host-served; a prefix rule that leaked would strand the
    // entire remote surface on local IPC.
    expect(findLocalOnlyCommand("list_tasks")).toBeUndefined();
    expect(findLocalOnlyCommand("update_notification_settings")).toBeUndefined();
  });

  it("does not match a name that merely CONTAINS the prefix", () => {
    expect(isPluginCommand("install_plugin:opener")).toBe(false);
    expect(findLocalOnlyCommand("install_plugin:opener")).toBeUndefined();
  });
});

describe("the host-targeted exception list", () => {
  it("is empty — the Phase 2 review found no plugin call whose subject is the host", () => {
    // A reviewed empty list, not an absent one. Adding a name here is a deliberate act that
    // removes it from local-only classification entirely, which is what the next case proves.
    expect(HOST_TARGETED_PLUGIN_COMMANDS).toEqual([]);
  });

  it("an excepted name would leave local-only classification, not sit in a grey zone", () => {
    // Falsifies the exception mechanism without weakening the shipped policy: the same
    // predicate the rule uses, evaluated against a one-name exception set. An excepted name
    // must then earn a facade registration or a ledger disposition — the P-11 drift scan
    // reports it unclassified otherwise.
    const excepted = new Set(["plugin:opener|open_url"]);
    const classify = (cmd: string) =>
      isPluginCommand(cmd) && !excepted.has(cmd) ? "run-locally" : undefined;

    expect(classify("plugin:opener|open_url")).toBeUndefined();
    expect(classify("plugin:opener|open_path")).toBe("run-locally");
  });
});
