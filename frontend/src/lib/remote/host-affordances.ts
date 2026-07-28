/**
 * Copy and predicates for HOST-IMPOSSIBLE affordances (PR 2.6-a).
 *
 * These gates read NOTHING but the active environment's kind. They are not
 * permission checks: no scope, no manifest, and no host round-trip can make a
 * remote client open a Finder window on the host Mac or pick a folder from a
 * filesystem it cannot see. Granting `ui:agent` changes none of them.
 *
 * The hide-vs-disable split is deliberate and load-bearing:
 *
 * - HIDE when the affordance's entire purpose is host-local and there is nothing
 *   honest to say about it in a remote session (project creation, the folder
 *   picker, terminal entry points). A disabled control with a tooltip nobody can
 *   act on is UI debt, not honesty.
 * - DISABLE + explain when the affordance names a real thing the user can still
 *   reason about — a file that exists, an editor that would open it — but only on
 *   the other machine. Here the tooltip IS the information.
 *
 * What is never acceptable: leaving a control that looks live and throws. The
 * transport already rejects these commands with `REMOTE_COMMAND_UNAVAILABLE`
 * (`local-only-commands.ts`, `reject` disposition); 2.6-a's job is to make sure a
 * user never reaches that rejection by clicking something that looked enabled.
 */

/** Disabled-control copy for editor / file-manager / reveal affordances. */
export const HOST_ONLY_AFFORDANCE_HINT = "Available on the host Mac";

/** Tooltip on the copy action that replaces a clickable local file link. */
export const HOST_PATH_COPY_HINT = "Copy path — file is on the host";

/** Placeholder line on a remote chat attachment card. */
export const HOST_ATTACHMENT_HINT = "Stored on the host";

/** Accessible label for the copy-path action on a remote file reference. */
export const HOST_PATH_COPY_LABEL = "Copy host file path";
