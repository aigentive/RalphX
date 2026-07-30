# Using Codex with RalphX

Enable and check the Codex CLI that RalphX uses for agent work. At the end, you will know whether the CLI is ready, which model and effort RalphX will use, and how approval, sandboxing, Fast, and Ultra affect your runs.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Keep RalphX MCP tools available

RalphX MCP tools currently require Codex to run with Never approval and Danger Full Access.

1. Open Settings → **Models & Providers** → **Providers** → **Codex**.

   Expand the provider details to review the Codex runtime settings.

   RalphX shows the approval and sandbox values for the current provider configuration.

2. Read this requirement before choosing a more restrictive Codex configuration.

   In plain language, tighter approval or sandbox settings can be safer for a Codex run, but RalphX's MCP tools will no longer be available to that run.

   Keep `never` and `danger-full-access` when the run needs RalphX MCP tools.

   Choose a hardened configuration only when you accept that trade-off.

3. Inspect **Approval Policy** and **Sandbox Mode** in the Codex details.

   Approval policy controls when Codex asks before an action: `never` does not ask, `on-request` lets Codex request approval, `on-failure` asks after a failed attempt, and `untrusted` asks for actions it does not trust.

   Sandbox mode controls the run's file and system access: `danger-full-access` is unrestricted, `workspace-write` permits changes in the workspace, and `read-only` prevents write access.

   These controls are Codex-specific; set the pairing deliberately for the work you plan to run.

## Understand when Codex Fast is unavailable

1. Turn on **Fast mode** only when its switch is available.

   Codex Fast uses the Codex priority service tier.

   RalphX reports a specific reason whenever it cannot enable Fast, so use that reason instead of retrying the switch blindly.

2. Enable the provider when RalphX says “Enable Codex in Settings.”

   Turn on **Enabled** for the **Codex** card, then return to **Fast mode**.

3. Update or repair the CLI when RalphX says “Missing CLI support: …”.

   The installed Codex CLI is missing required execution features.

   Use the managed CLI action when available, or install a CLI version that provides the missing feature, then re-check the card.

4. Fix CLI readiness when RalphX shows its validation error or “Codex CLI validation failed.”

   Read the **CLI Ready** or **CLI Not Ready** status and the resolved binary path on the **Codex** card.

   Correct the CLI installation, authentication, or path issue named in that status, then re-check it.

5. Update the CLI or choose a supported model when RalphX says “Fast mode is not available for this Codex CLI or model catalog.”

   This Codex CLI or the catalog it reports does not support Fast.

   Update Codex, then review the model selection again.

6. Choose a supported model when RalphX says “Fast mode is not available for &lt;model&gt;.”

   The active model is not in the Fast-supported model list advertised by this CLI.

   Choose another available model or turn Fast off for this run.

7. Wait when RalphX says “Checking Codex Fast support.”

   RalphX is still checking the provider and CLI capabilities.

   Let the check finish before changing settings.

## Enable and maintain the Codex CLI

1. Check the badge and CLI status on the **Codex** card.

   The badge reads **Enabled**, **Ready**, or **Not ready**.

   The CLI row reads **CLI Ready** or **CLI Not Ready** and displays the binary path RalphX resolved.

   Enable Codex only after its CLI is ready.

2. Follow **Install instructions** when the CLI is not ready.

   The link opens the [Codex installation instructions](https://help.openai.com/en/articles/11096431).

   Install and authenticate Codex there, then return to RalphX and re-check the provider card.

3. Use RalphX-managed installation when you want RalphX to maintain the CLI.

   Turn on **Let RX manage this CLI** to allow RalphX-managed installs and updates without changing a Codex binary that you manage through your PATH.

   The management block is labeled **RX-managed CLI**, or **CLI update available** when a user-managed installation has an update.

   Click **Install Codex** when the managed CLI is absent, or **Update Codex** when an update is available.

4. Restore or apply provider defaults when you need a consistent setup.

   Click **Reset Codex** to restore Codex's built-in defaults.

   Click **Apply to all agents** to make Codex the default provider and update every agent lane to use its current defaults.

## Choose a model and effort

1. Choose a value in **Default Model** and **Default Effort** for the Codex provider.

   The model menu shows each shipped model's short description; treat that live menu as the catalog rather than relying on a fixed list in this guide.

   The GPT-5.6 family is shown only when the installed CLI advertises the matching model capability.

   An unavailable GPT-5.6 option therefore points to CLI capability, not a broken RalphX setting; update the CLI and re-check its status.

2. Choose an effort that the selected model supports.

   Codex offers Low, Medium, High, Extra High, and Max effort levels, with descriptions in the menu for the active model.

   **Ultra** is Codex-only and means “Maximum reasoning with automatic task delegation.”

   Ultra appears only when both the selected model declares Ultra support and the installed Codex CLI advertises support for that model.

3. Understand RalphX's fallback when a runtime selection is missing or invalid.

   RalphX normalizes that selection to Codex rather than leaving the runtime unset.

   It prefers `gpt-5.6-terra` when the CLI advertises it; otherwise it falls back to `gpt-5.5`.

   RalphX also chooses the selected fallback model's default effort, adjusted to an effort the provider supports.

## What you have now

Codex is enabled only after RalphX can find a ready CLI, and you know where to install or update it. You can choose a model and effort with the CLI's advertised capabilities in mind, and you understand the MCP access trade-off behind a hardened approval or sandbox configuration.

## Next

- [Teaching RalphX about your project](project-setup-and-validation.md)
