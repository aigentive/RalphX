# Connecting RalphX to Granola

Connect Granola so RalphX can find meeting notes and use them as context in a conversation. At the end, RalphX has a validated Granola API token, and you can bring notes into agent work without treating Granola as a task tracker.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Create a Granola API key

1. Open Settings → **Integrations** → **Granola**.

   The Granola card is labeled “Granola note references.”.

   Its status banner initially says “Note references not ready”.

2. Read the in-panel **Get a Granola API key** help block and follow it exactly.

   “Open the Granola desktop app, go to Settings -> Connectors -> API keys, create a new key, choose the note scopes, then paste it here.”.

   Use the linked [Granola API docs](https://docs.granola.ai/introduction) if you need more detail before creating the key.

   Copy the new API key from Granola.

   Choose note scopes that include the notes you expect RalphX to search. For the exact scope names available in Granola, see [Granola's API documentation](https://docs.granola.ai/introduction) — RalphX cannot verify Granola's scope names from its own source.

   Keep the key private after copying it.

## Save and validate the token in RalphX

1. Paste the API key into **API token**.

   The empty field says “Paste Granola API token”.

   Once a token is stored, the field instead says “Stored token unchanged”.

   The helper text says “Used for @granola note references and prompt context.”.

   RalphX stores the token you paste here and uses it for all Granola requests.

2. Click **Save API token**.

   A transient “Saved” confirmation means RalphX saved the token.

   Saved does not yet prove that the API key works with Granola.

   It is safe to leave the field unchanged when it says “Stored token unchanged”.

3. Click **Validate**.

   Validation checks the saved token with Granola.

   A successful connection changes the banner to “Note references enabled”.

   Keep the settings panel open until validation finishes.

   If validation fails, create or copy a usable Granola key with the note scopes you need, then save and validate it again.

## Recover from a validation error

1. Enter a value in **API token** before clicking **Save API token** if the panel says "Granola API token cannot be empty"

   The token field must contain text before RalphX can save it. Paste the API key you copied from the Granola desktop app.

2. Paste a fresh key and try again if the panel says "Failed to save Granola API token"

   This message means RalphX encountered a problem writing the token. Open the Granola desktop app, create a new API key in Settings → Connectors → API keys, paste it into **API token**, and click **Save API token**.

3. Click **Validate** if the panel says "Granola API token was saved, but note references are still disabled"

   Saving and validating are separate steps. The key is stored but RalphX has not yet confirmed that Granola accepts it. Click **Validate** to complete the check.

4. Create a replacement key in Granola if the panel says "Failed to validate Granola integration"

   Open the Granola desktop app and go to Settings → Connectors → API keys to create a new key. Choose note scopes that cover the meetings you expect RalphX to search. Paste the new key into **API token**, click **Save API token**, then click **Validate**.

## Use Granola notes as context

1. Open the top-level **Granola** navigation view to work with connected notes.

   Granola is a notes source, not a tracker.

   Use it for meeting decisions, discussion context, and details that would otherwise stay in notes, rather than for task or issue management.

   Use your issue tracker when you need task assignment, status, or delivery workflow.

   Keeping notes and tracker work distinct makes it clearer why you are adding a Granola reference to a conversation.

2. In an agent conversation, type an `@granola:` reference followed by the note you want to find.

   RalphX uses the saved token to search Granola notes and add the selected note as prompt context.

   Choose the note that gives the agent the relevant decisions or discussion before asking it to plan or change code.

   This lets the conversation use meeting context without copying the note text by hand.

   Start with the note most directly related to the decision or work under discussion.

## Remove the Granola connection

1. Return to Settings → **Integrations** → **Granola** and click **Disconnect**.

   RalphX first asks you to confirm the removal.

   The first click does not remove the token.

2. Click **Confirm disconnect** to remove the stored token, or click **Cancel** to keep the connection.

   Choose **Confirm disconnect** only when you no longer want RalphX to use this Granola account.

   You can reconnect later by creating a new API key in Granola, then saving and validating it again.

   Disconnecting does not remove any notes from Granola.

## What you have now

RalphX stores and has validated a Granola API token for the notes available to your account. You can open the **Granola** view and use `@granola:` references to bring meeting notes and their decisions into an agent conversation.

## Next

- [Finding your way around RalphX](../02-tour-of-the-app.md)
