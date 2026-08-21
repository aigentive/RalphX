# Connecting RalphX to ClickUp

Connect ClickUp so RalphX can find tasks for the workspace you choose and use them as conversation context. At the end, you have validated a personal token and selected the ClickUp workspace whose tasks RalphX should load.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Create a ClickUp personal API token

1. In ClickUp, click your avatar in the lower-left corner and select Apps.

   Under API Token, click Generate and copy the resulting personal API token.

   [ClickUp's API documentation](https://help.clickup.com/hc/en-us/articles/6303422883095-Create-your-own-app-with-the-ClickUp-API) confirms that a personal token uses the same access as your ClickUp account.

   No scope selection is needed when you generate the token — a personal token inherits your full ClickUp account permissions automatically.

   Use a personal token, not an OAuth client ID or client secret.

   The RalphX panel does not include these token-creation instructions; they are a ClickUp-side setup step.

2. Keep the token private.

   The token can access the ClickUp workspaces available to your account.

   Generate a replacement in ClickUp if you need to revoke the token later.

## Save and validate the token in RalphX

1. Open Settings → **Integrations** → **ClickUp**.

   The ClickUp card is labeled “ClickUp ticket references.”.

   Its status banner initially says “Task references not ready”.

2. Paste the token into **API token**.

   The empty field says “Paste ClickUp personal API token”.

   Once a token is stored, the field instead says “Stored token unchanged”.

   The helper text says “Used for ClickUp ticket search and the unified ticketing dashboard.”.

   RalphX stores the token you paste here and uses it for all ClickUp requests.

3. Click **Save API token**, then click **Validate**.

   A transient “Saved” confirmation means RalphX saved the token.

   It does not complete this integration: validation must succeed before RalphX can show the workspace selector.

   When validation succeeds, the banner changes to “Task references enabled”.

## Recover from a validation error

1. Enter a value in **API token** before clicking **Save API token** if the panel says "ClickUp API token cannot be empty"

   The token field must contain text before RalphX can save it. Paste the personal API token you copied from ClickUp.

2. Paste a fresh token and try again if the panel says "Failed to save ClickUp API token"

   This message means RalphX encountered a problem writing the token. Paste a new personal API token from ClickUp into **API token**, click **Save API token**, then click **Validate**.

3. Click **Validate** if the panel says "ClickUp API token was saved, but task references are still disabled"

   Saving and validating are separate steps. The token is stored but RalphX has not yet confirmed that ClickUp accepts it. Click **Validate** to complete the check.

4. Return to ClickUp and generate a new personal API token if the panel says "Failed to validate ClickUp integration"

   Paste the new token into **API token**, click **Save API token**, then click **Validate**.

5. Click **Validate** again and then choose the workspace if the panel says "Failed to select ClickUp workspace"

   The workspace selector requires a confirmed token. Re-validating reloads the workspace list so you can complete the selection.

## Select the ClickUp workspace

1. After a successful validation, choose a value in **Workspace**.

   The **Workspace** selector appears only after **Validate** succeeds.

   If it is loading, it reads “Loading workspaces...”; otherwise its initial option is “Select a workspace”.

   This is a required second step. Saving a token without selecting a workspace leaves RalphX with no ClickUp tasks to load.

2. Choose the workspace whose tasks you want RalphX to load.

   The helper text says “Tasks load from the Spaces in the selected ClickUp Workspace.”.

   Select the workspace that contains the Spaces relevant to your RalphX project.

   Return here to choose a different workspace if your task context should come from another ClickUp workspace.

## Use ClickUp tasks as context

1. In an agent conversation, type an `@clickup:` reference followed by the task you want to find.

   RalphX uses the saved token and selected workspace to search ClickUp tasks and add the selected task as prompt context.

   Use this when one task's status, description, or linked work should shape the conversation.

2. Open the **Ticketing** view to browse the connected ticketing data.

   ClickUp tasks load there from the Spaces in the workspace you selected.

   If expected tasks are missing, first confirm the selected **Workspace** and its Spaces.

## Remove the ClickUp connection

1. Return to Settings → **Integrations** → **ClickUp** and click **Disconnect**.

   RalphX first asks you to confirm the removal.

   The first click does not remove the token.

2. Click **Confirm disconnect** to remove the stored token, or click **Cancel** to keep the connection.

   Disconnecting removes RalphX's saved ClickUp credential and workspace selection.

   You can reconnect later by saving and validating a personal API token, then completing the required **Workspace** selection.

## What you have now

RalphX stores and has validated your ClickUp personal API token, and it knows which ClickUp workspace to load. You can use `@clickup:` references for conversation context and use the **Ticketing** view to work with tasks from that selected workspace.

## Next

- [Finding your way around RalphX](../02-tour-of-the-app.md)
