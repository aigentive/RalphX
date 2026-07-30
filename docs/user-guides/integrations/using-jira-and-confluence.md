# Using Jira and Confluence in RalphX

Bring Jira issues and Confluence pages into a conversation so the agent can plan and implement from the same ticket and knowledge-base context you use. At the end, you can reference content without copying its details into the composer by hand.

**Before you start:** [Connecting RalphX to Jira and Confluence](connect-jira-and-confluence.md)

## Reference a Jira issue in the composer

1. Type a Jira reference in the composer with `@jira:KEY`.

   Replace `KEY` with the issue key, such as `@jira:PROJ-123`.

   RalphX normalizes Jira keys, so you can use the issue key you see in Jira.

2. Add the reference to the message that starts or continues the work.

   Use the issue reference when the agent needs the ticket's context while it plans or implements.

   Keep your request focused on the outcome you want; the referenced issue supplies the supporting ticket context.

3. Send the message after checking that the key names the intended issue.

   A Jira reference can establish the conversation's primary Jira issue when one has not already been assigned.

   The primary issue keeps the conversation tied to the ticket that anchors the work.

## Reference a Confluence page in the composer

1. Type `@confluence:query` to find or reference Confluence content.

   Replace `query` with terms that identify the page or content you want the agent to use.

   Use a specific title, project name, or distinctive phrase when several pages may match.

2. Use `@conf:query` as the shorter form when you prefer it.

   `@conf:` and `@confluence:` both identify Confluence references.

   Include the query after the colon; a prefix by itself has no page context to resolve.

3. Add a short instruction beside the reference.

   For example, ask the agent to apply a documented decision, compare a proposed change with a page, or turn an approved specification into an implementation plan.

   This keeps the purpose of the reference clear without pasting the page's full text into the conversation.

## Paste a Jira or Confluence URL

1. Paste a Jira issue URL or Confluence-page URL into the composer.

   RalphX recognizes supported URLs from your connected Atlassian site and tries to resolve them into a reference.

   Use this when you have already opened the issue or page in your browser and do not want to type a reference prefix.

2. Wait for RalphX to resolve the pasted URL before sending the message.

   A resolved URL becomes selected Atlassian context instead of remaining as pasted text in the composer.

   If the integration is disconnected, invalid, or the content is inaccessible, RalphX leaves the pasted URL unchanged.

3. Add your request and send it when the selected context is correct.

   Paste one or more supported URLs when the agent needs related tickets or documentation together.

   Use only content that the connected Atlassian account is permitted to access.

## Work from the conversation's primary Jira issue

1. Start the conversation with the Jira issue that should anchor the work.

   When the conversation has no primary Jira issue yet, RalphX assigns the first Jira reference as its primary issue.

   Later Jira references can supply more context without replacing that primary issue.

2. Review the primary issue's details in the conversation workspace.

   RalphX refreshes the issue's title, status, assignee, reporter, description, and acceptance criteria when the connected Jira resource is available.

   Use these details to keep planning and implementation aligned with the ticket.

3. Read the issue's comments and attachments when they are present.

   Comments can explain recent decisions or clarifications that are not in the description.

   Attachments can provide supporting material without requiring you to copy it into the message.

4. Keep the issue as the source of truth while you discuss the change.

   The agent receives the ticket context with your request, which reduces repeated copy-paste between Jira, Confluence, and RalphX.

## Browse connected Jira tickets

1. Open **Ticketing** from the navigation rail.

   The Ticketing view is available when a ticketing provider is connected and ready.

   Choose Jira there to browse tickets from the connected Atlassian site.

2. Open the ticket you want to understand before starting a conversation.

   Use the ticket key from the view in an `@jira:KEY` reference, or paste its Jira URL into the composer.

   This gives you two ways to bring the same Jira context into planning or implementation.

3. Return to the conversation when you are ready to ask for work.

   Reference the relevant Jira issue and any Confluence page that explains the expected outcome.

## What you have now

You can bring Jira issues and Confluence pages into a RalphX conversation with reference prefixes or pasted URLs. A primary Jira issue can carry its details, comments, and attachments with the conversation while you plan and implement.

You can also browse connected Jira tickets from the Ticketing view, then attach the right ticket context to the work without copying it by hand.

## Next

- [Planning a feature with RalphX](../workflows/planning-a-feature.md)
