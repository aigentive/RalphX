# Connecting other tools to RalphX

Give a local tool or automation controlled access to RalphX through an API key and the External MCP server. At the end, you will have a scoped key and the connection details your third-party client needs.

**Before you start:** [Finding your way around](../02-tour-of-the-app.md)

## Create a scoped API key

1. Open Settings → **Integrations** → **API Keys**.

   API keys let external tools access RalphX programmatically.

   Create a separate key for each tool or automation so you can identify and remove it independently later.

2. Click **Create API Key**.

   Enter a recognizable **Key Name** for the client or environment that will use the key.

3. Set **Project Access** for the projects this client should reach.

   Limit the key to only the projects the client needs.

   Leave this optional field unselected only when the client genuinely needs access to every project.

   Review the selected projects before creating the key, especially if you switch between personal and team work.

4. Choose the needed **Permissions**.

   Give the key the smallest set of permissions that lets the client complete its work.

   Use a more limited key for a read-only integration than for a tool that must make changes.

   You can create another key later when a different client needs a different scope.

5. Click **Create Key** and copy the value shown under **Your API Key**.

   RalphX shows the complete key once only.

   Store it in the third-party client's secure credential store before closing the dialog; you cannot retrieve the complete value again.

## Enable the External MCP server

1. Open Settings → **Integrations** → **External MCP**.

   This panel controls the local server that accepts connections from external MCP clients.

2. Turn on **Enabled**.

   Confirm the **Host** and **Port** values before connecting a client.

   The default local address is `127.0.0.1` and the default port is `3848`.

3. Click **Save** after changing the server settings.

   All settings in this panel require an app restart to take effect.

   Restart RalphX before testing the client connection.

   A client cannot connect until the restart has applied the enabled server configuration.

## Connect your third-party client

1. Configure the client to use this local MCP endpoint:

   `http://127.0.0.1:3848/mcp`

   Keep `127.0.0.1` when the client runs on the same Mac as RalphX.

   If the client runs elsewhere, review the security guidance before changing the host.

2. Add this HTTP header to the client's MCP connection:

   `Authorization: Bearer rxk_live_<your_key>`

   Replace `<your_key>` with the full API key you copied, including its `rxk_live_` prefix.

   Do not paste the key into a conversation, source file, or shared configuration.

3. Test the connection from the client using only the projects and permissions selected for its key.

   If access is denied, first confirm that the key is present and that its project scope and permissions cover the requested action.

   Create a new key if you no longer have the original complete value.

## Go deeper when you need to

1. Read the [External MCP overview](../../external-mcp/README.md) before configuring advanced client behavior.

   It documents the protocol surface and operational details that do not belong in this quick connection guide.

2. Read the [External MCP security model](../../external-mcp/security-model.md) before exposing the server beyond your local Mac.

   Treat an API key as a credential and keep its project access and permissions narrowly scoped.

## What you have now

You have a one-time-revealed API key scoped to the projects and permissions your external client needs. The External MCP server can accept that client's authenticated local connection after RalphX restarts.

## Next

- [When something goes wrong](../troubleshooting.md)
