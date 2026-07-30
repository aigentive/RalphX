# Connecting RalphX to Jira and Confluence

Connect your Atlassian site so RalphX can retrieve Jira issues and Confluence pages as context for a conversation. At the end, the connection is saved and checked for both Jira and Confluence.

**Before you start:** [Installing RalphX and running it for the first time](../01-install-and-first-run.md)

## Open the Atlassian connection settings

1. Open Settings → **Integrations** → **Atlassian**.

   Use the Atlassian page to enter the credentials for the site whose Jira issues and Confluence pages you want RalphX to use.

   If you skipped the optional Atlassian step during first-run onboarding, you can complete the same setup here at any time.

2. Enter your Atlassian base address in **Site URL**.

   Use the URL for your site, such as `https://your-team.atlassian.net`.

   Do not paste an individual Jira issue or Confluence-page URL into this field.

3. Enter the email address for your Atlassian account in **Account email**.

   Use the account that owns the API token you will enter next.

   The email and token must belong to an account that can access the Jira and Confluence content you plan to reference.

## Create and enter an API token

1. Click **Open Atlassian API tokens**.

   RalphX supplies this link in the connection panel, and it opens Atlassian's API-token management page.

   Create the token in your Atlassian account, following the instructions on [Atlassian's API-token page](https://id.atlassian.com/manage-profile/security/api-tokens).

   Copy the token when Atlassian shows it. Treat it like a password and do not share it in a conversation or project file.

2. Return to RalphX and paste the token into **API token**.

   RalphX combines this token with the **Site URL** and **Account email** you entered.

   If the field says that a token is already stored, leave it unchanged unless you intend to replace it.

3. Click **Save and validate**.

   RalphX saves the values and checks the connection before marking it ready.

   Wait for the result before using Jira or Confluence references in a conversation.

4. Correct the site URL, account email, or API token and click **Save and validate** again if validation does not succeed.

   Start by confirming that the account can open both Jira and Confluence in your browser.

   A token can only provide access that the Atlassian account already has.

## Check what validation covers

1. Wait for RalphX to finish checking both services after you click **Save and validate**.

   A successful result means the credentials can be used for the connected site.

   It does not grant access to Jira projects or Confluence spaces that your Atlassian account cannot already open.

2. Update the credentials from this page if your Atlassian administrator changes the account's access.

   Enter a replacement token in **API token** when the current one is revoked or expires.

   Keep the same **Site URL** only when the replacement account belongs to the same Atlassian site.

3. Use **Save and validate** after every credentials change.

   Saving without validation records the values, but validating confirms they work for both sources before you rely on them in a conversation.

## Finish a skipped onboarding step

1. Use the optional **Atlassian** step on the first-run screen when you want to connect before creating your first project.

   That step is labelled optional because RalphX works without Atlassian context.

   It is a shortcut to this connection setup, not a requirement for using RalphX.

2. Return to Settings → **Integrations** → **Atlassian** whenever you need to review or update the connection.

   Save and validate again after replacing an expired or revoked token.

## Remove the connection

1. Click **Disconnect** on the Atlassian settings page.

   The first click only opens the removal confirmation, so your saved connection is still present at this point.

2. Click **Confirm disconnect** to remove the saved Atlassian connection.

   RalphX clears the connection, so Jira and Confluence content is no longer available for new references until you connect again.

3. Click **Cancel** instead when you decide to keep the connection.

   Cancel closes the confirmation without removing the saved settings.

## What you have now

RalphX has a saved and validated connection to your Atlassian site. Jira issues and Confluence pages that your Atlassian account can access are ready to become conversation context.

You can reconnect from the same settings page whenever your site address, account, or API token changes.

## Next

- [Using Jira and Confluence in RalphX](using-jira-and-confluence.md)
