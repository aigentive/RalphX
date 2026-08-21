# Using Claude Code with RalphX

Set up Claude Code as a RalphX provider, confirm that its local CLI is ready, and choose the defaults your agents use. At the end, Claude is enabled, its authentication is verified through your existing local setup, and you understand which model and effort choices RalphX can offer.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Check the Claude CLI

1. Open Settings → **Models & Providers** → **Providers** → **Claude**.

   The card is RalphX's view of the `claude` command available on your Mac.

   RalphX shells out to that local CLI and reuses its existing authentication.

   RalphX stores no Anthropic credential.

![The RalphX Providers settings showing the Claude provider card and its status badges](../../../assets/public/guides/providers-claude-card.png)

2. Read the status badge and CLI status on the **Claude** card.

   **Enabled** means RalphX can use Claude for agents.

   **Ready** means the CLI is available but not enabled for agents yet.

   **Not ready** means RalphX cannot currently use the CLI.

   **CLI Ready** also shows the resolved binary path, so you can confirm which local installation RalphX found.

   **CLI Not Ready** means the command needs installation or repair before you can enable it.

3. Follow **Install instructions** when the CLI is not ready.

   The link opens Anthropic's Claude Code setup instructions.

   Install and authenticate Claude Code there, then return to this card and check the status again.

4. Use RalphX-managed installation only when you want RalphX to maintain a separate CLI installation.

   Turn on **Let RX manage this CLI** to allow RalphX-managed installation and updates without changing a CLI that your terminal already finds on `PATH`.

   The managed block reads **RX-managed CLI** when RalphX manages it and offers **Install Claude** when an installation is needed.

   When a user-managed installation has an update available, the block reads **CLI update available** and offers **Update Claude**.

   Leave the switch off if you want to keep managing your own CLI installation.

## Enable Claude for agents

1. Turn on **Enabled** on the **Claude** card after its status says **CLI Ready**.

   The provider controls whether RalphX can select Claude for agent work.

2. Click **Apply to all agents** when you want the card's current provider defaults to become the defaults for every agent.

   Use this after choosing the model, effort, and permission settings you want to share.

3. Click Reset Claude to restore the Claude card's defaults.

   Resetting is useful when you want to discard the card's local choices before applying a new shared configuration.

## Choose a permission mode deliberately

1. Open **Show permissions** on the enabled **Claude** card, then choose a **Permission Mode**.

   The mode controls how Claude Code handles requests to use tools during RalphX-launched runs.

2. Choose the mode that matches how much review you want before actions run.

   **default** is the cautious starting point: reads run without asking, while other actions ask first.

   **acceptEdits** also accepts file edits and common filesystem operations, while retaining prompts for more consequential actions.

   **plan** is for codebase exploration and planning; it permits reading but not edits or commands.

   **dontAsk** permits only pre-approved tools, which suits locked-down automated work where blocked actions are preferable to prompts.

   **auto** reduces prompts by allowing actions through Claude Code's background safety checks; use it only when you trust the task's general direction and still plan to review results.

   **bypassPermissions** skips permission prompts. Reserve it for isolated environments where you accept the highest level of operational risk.

3. Turn on **Skip Permissions** only if you intend RalphX-launched Claude runs to bypass Claude permission prompts.

   This switch is separate from the selected mode and has a high-risk effect.

## Select a model and effort

1. Choose a value in **Default Model**.

   Use an alias such as `sonnet`, `opus`, `haiku`, or `fable` when you want Claude Code to resolve the current model behind that alias.

   `sonnet` is RalphX's default Claude alias.

   Exact model IDs are also available when your installed CLI supports them; treat entries such as `claude-sonnet-5` or `claude-opus-5` as examples, not a permanent catalog.

2. Choose a value in **Default Effort** that the selected model supports.

   **Low** is for short, scoped, latency-sensitive work that is not intelligence-sensitive.

   **Medium** reduces token use for cost-sensitive work while trading off intelligence.

   **High** balances token use and intelligence; use at least this level for intelligence-sensitive work.

   **Extra High** is Claude's best setting for most coding and agentic use cases.

   **Max** is for intelligence-demanding work that justifies higher usage and possible overthinking.

   **Ultra** is not a Claude effort level, so it does not appear for Claude models.

## Understand why an exact model is missing

1. Treat an absent exact model ID as a CLI-version signal, not a broken RalphX setting.

   Each exact Claude model carries its own minimum Claude Code version.

   RalphX offers one only when the installed CLI advertises the matching model alias.

   For example, the availability floor for `claude-opus-5` is newer than the floor for `claude-opus-4-8`.

2. Click **Update Claude** on the same card when an exact model you expect is missing.

   After the update, return to **Default Model** and check the dropdown again.

   If the CLI now advertises the matching alias, RalphX makes the exact ID selectable.

   This check keeps the selector aligned with what the installed Claude Code version can actually run.

## What you have now

Claude Code is enabled and verified through the local `claude` CLI that you already authenticate on your Mac. You have chosen provider-wide defaults for permissions, model selection, and effort, and you know that an unavailable exact model usually calls for a Claude CLI update.

## Next

- [Finding your way around](../02-tour-of-the-app.md)
