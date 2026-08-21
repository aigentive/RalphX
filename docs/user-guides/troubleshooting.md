# When something goes wrong

Recover from common RalphX setup and run problems without guessing. This guide helps you check the local prerequisites, retry the right step, find logs, and send a useful issue report when self-service recovery is not enough.

**Before you start:** [Installing RalphX and running it for the first time](01-install-and-first-run.md)

## Your agent runtime provider is not authenticated or ready

1. Open Settings → **Models & Providers** → **Providers**.

   Check the provider you intend to use.

   A provider must be installed, authenticated in its own CLI, and available before RalphX can run it.

2. Use the provider's status message to correct the local CLI setup.

   When the panel shows **CLI Not Ready**, install or repair the provider CLI, then authenticate it in a terminal using that provider's normal sign-in flow.

   Open a new terminal after changing CLI installation or authentication so the updated command and credentials are available.

3. Return to **Providers** and confirm the provider is ready before starting another agent run.

   If the provider is available but not selected for use, enable it in the same panel.

   Do not retry a run until the provider reports a usable state; repeated attempts cannot bypass missing local authentication.

## The GitHub CLI is not signed in

1. Open Settings → **Integrations** → **GitHub** and check the connection status.

   RalphX uses the local GitHub CLI rather than storing a GitHub token in the app.

2. Follow [Connecting RalphX to GitHub](integrations/connect-github.md) to install `gh`, run `gh auth login`, and refresh the status in RalphX.

   This restores GitHub-dependent work such as pull-request information and reviews.

3. Click **Refresh** after the terminal sign-in is complete.

   If the status is still not authenticated, run `gh auth login` again and complete the browser or device approval it opens.

## Project setup or validation fails

1. Open Settings → **Repository** → **Setup & Validation**.

   Read the failed command and its output before changing the project configuration.

2. Follow [Teaching RalphX about your project](configure/project-setup-and-validation.md) to correct the command, working directory, or prerequisites it needs.

   Run the validation again only after fixing the reported problem.

3. Keep setup and validation commands small and reproducible.

   A command that succeeds in a terminal but fails in RalphX often depends on a missing tool, environment value, or project-specific setup step.

## An agent run appears stuck

1. Give the run a moment to finish its current tool or provider operation.

   Long-running commands and provider responses can take time even when the conversation has no new visible message.

2. Check the latest conversation messages for a question, error, or request for input.

   Answer a pending question or resolve the reported local prerequisite before expecting the run to continue.

3. If the run exposes **Cancel**, stop it rather than leaving it open indefinitely.

   Start a new **Agent** conversation only after you know what prevented the original run from progressing.

   Include the original goal and the relevant error in the new request, but do not include credentials or API keys.

4. Use the logs and issue-report steps below if the run repeatedly stops progressing without an actionable message.

   Repeatedly starting new runs without changing the underlying setup usually repeats the same failure.

## Find the app logs

1. In Finder, open `~/Library/Application Support/com.ralphx.app/logs/`.

   This is the release-build location for RalphX backend and runtime logs on macOS.

2. Inspect the newest log around the time the problem occurred.

   Look for the provider, project setup command, or connection error that matches your symptom.

3. Remove or redact credentials before sharing log excerpts.

   API keys, access tokens, and private project data should stay private even when you need help.

## Report an issue with useful context

1. Keep the affected agent conversation selected and click **Report Issue** in the left navigation rail.

   RalphX prepares context from the selected conversation so you can review it before creating the GitHub issue.

2. Describe what you expected, what happened, and the steps that reproduce it.

   Include the time of the failure and any relevant redacted log excerpt.

3. Remove secrets and unrelated project data from the report before submitting it.

   Do not include API keys, tokens, passwords, or private source code unless you have explicitly decided it is safe to share.

## What you have now

You know how to restore provider and GitHub CLI access, correct project setup or validation failures, and stop an agent run that is not progressing. You also know where to find the macOS logs and how to report a problem with the relevant conversation context.

## Next

- [Connecting other tools to RalphX](advanced/external-access.md)
