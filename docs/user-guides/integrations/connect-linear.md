# Connecting RalphX to Linear

Connect your Linear account so you can bring relevant issues into a RalphX conversation and see them in the ticketing workspace. At the end, RalphX has a validated Linear token and can search the issues available to that account.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Create a Linear API token

1. Open Linear and go to Settings → Account → Security & Access.

   Linear documents this as the place where admins and permitted members create personal API keys.

   Create a personal API key that can read the teams and issues you want RalphX to use.

   Follow [Linear's API and webhooks documentation](https://linear.app/docs/api-and-webhooks) if your workspace controls who can create keys or you need to choose permissions.

   Copy the new key before leaving Linear.

   RalphX does not show these token-creation instructions in its panel; this is a Linear-side setup step.

2. Keep the token private.

   A Linear API key acts with the access granted to the Linear account that created it.

   Create a new key specifically for RalphX if you want to revoke this access independently later.

   Avoid pasting the token into a conversation or project file.

   Enter it only in the Linear integration panel in Settings.

## Save and validate the token in RalphX

1. Open Settings → **Integrations** → **Linear**.

   The Linear card is labeled “Linear issue references.”

   Its status banner initially says “Issue references not ready.”

2. Paste the token into **API token**.

   The empty field says “Paste Linear API token.”

   Once a token is stored, the field instead says “Stored token unchanged.”

   The helper text says “Used for @linear issue search and prompt context.”

   RalphX stores this token itself. This differs from the GitHub integration, which delegates authentication to the local `gh` CLI instead of storing a token in RalphX.

3. Click **Save API token**.

   A transient “Saved” confirmation appears after RalphX saves the token.

   Saved means RalphX has kept the token; it does not yet prove that Linear accepts it.

4. Click **Validate**.

   Validation checks the saved token with Linear.

   A successful connection changes the banner to “Issue references enabled.”

   Keep the Linear settings panel open until the result settles.

   If validation fails, return to Linear, create or copy a usable personal API key, replace the value in **API token**, and save and validate again.

## Use Linear issues as context

1. In an agent conversation, type an `@linear:` reference followed by the issue you want to find.

   RalphX uses the saved token to search Linear issues and add the selected issue as prompt context.

   Use the issue that best describes the work you want the agent to understand, rather than copying its details manually.

2. Open the **Ticketing** view to browse the connected ticketing data.

   Linear issues can appear there alongside other ticket sources.

   The view helps you orient around work, while the `@linear:` reference supplies a specific issue to a conversation.

   Use the issue title and status to confirm that RalphX can reach the Linear work you expect.

## Remove the Linear connection

1. Return to Settings → **Integrations** → **Linear** and click **Disconnect**.

   RalphX first asks you to confirm the removal.

   The first click does not remove the token.

2. Click **Confirm disconnect** to remove the stored token, or click **Cancel** to keep the connection.

   Choose **Confirm disconnect** only when you no longer want RalphX to use this Linear account.

   You can reconnect later by creating or using a Linear API key, then saving and validating it again.

   Disconnecting does not delete the issue data in Linear.

## What you have now

RalphX stores and has validated a Linear API token for the Linear account you chose. You can use `@linear:` references to bring issues into conversations and use the **Ticketing** view to work with connected issue data.

## Next

- [Finding your way around RalphX](../02-tour-of-the-app.md)
