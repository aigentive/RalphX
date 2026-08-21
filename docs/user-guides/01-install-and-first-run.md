# Installing RalphX and running it for the first time

Install the signed Mac app, connect an authenticated agent runtime, and create the first project RalphX will work in. At the end you will have RalphX open with a project ready for the next guide.

**Before you start:** nothing — this is the first guide

## Check the requirements

1. Confirm your Mac runs macOS 13 Ventura or later.

2. Install one supported agent runtime: the [Claude CLI](https://docs.anthropic.com/en/docs/claude-code) or the [Codex CLI](https://developers.openai.com/codex/cli).

   This is your *agent harness* — the AI runtime RalphX drives. RalphX is not itself a model; it launches and supervises the CLI you install here. See [RalphX concepts](concepts.md) if that distinction is new.

   One is enough. You can add the other later, and you can change which one each agent uses without reinstalling anything.

3. Authenticate that CLI in Terminal before opening RalphX.

   Follow the CLI's own login instructions. RalphX stores no credential of its own — it reuses the session already in your `claude` or `codex` CLI, so a CLI that cannot run on its own cannot run under RalphX either.

## Install RalphX with Homebrew

1. Add the RalphX tap and install the signed app in Terminal.

   ```sh
   brew tap aigentive/ralphx
   brew install --cask ralphx
   ```

2. Open RalphX from Applications or Spotlight once Homebrew finishes.

   For upgrades, stale tap metadata, repair, and uninstalling later, use the [Homebrew installation guide](../install/homebrew.md) rather than repeating these commands.

### If you do not use Homebrew

1. Download the signed build for your Mac from the [RalphX GitHub Releases page](https://github.com/aigentive/ralphx.app/releases).

2. Move the downloaded app into Applications, then open it.

   Homebrew is still the easier path for future upgrades. You can switch to it later without reinstalling from scratch.

## Set up your provider

1. Click **Set Up Provider** on the welcome screen.

   The **Provider** step is the first of the welcome screen's steps, and it reads “Choose your agent harness.” until a provider is ready.

![The Provider step of the RalphX welcome screen](../../assets/public/guides/welcome-provider-step.png)

2. Select the provider matching the CLI you installed, and complete the setup RalphX shows.

3. Confirm the **Provider** step now reads “Agent harness ready.”

   Do not continue until it does. Agent work cannot start without an authenticated provider, and every later guide assumes this step succeeded.

## Create your first project

1. Click **Start Your First Project** on the welcome screen.

   This is the same button you used a moment ago — its label changes as you progress. If you already have a project, it reads **Continue** instead.

2. Choose how to start in the **Create New Project** dialog.

   **Add Existing Repository** points RalphX at a folder already under version control. This is the usual choice.

   **Clone Repository** copies a remote repository onto your machine first.

   **Create New Repository** starts a brand-new project in an empty folder.

![The RalphX Project Creation Wizard](../../assets/public/guides/project-creation-wizard.png)

3. Click **Browse** beside **Location** and choose the project folder.

   **Location** is the only required field in the dialog; RalphX marks it with an asterisk and inspects the folder once you set it.

4. Leave **Project Name** empty, or type a short name.

   The field is optional. Left empty, RalphX infers the name from the folder. Set it when you want a label that is easier to recognize among several projects.

5. Set **Base branch** under **Git Settings**.

   This is the branch your work starts from and is meant to merge back into — normally your team's integration branch, such as `main`, not a feature branch.

6. Open **Advanced Settings** and check **Worktree Parent Directory**.

   RalphX creates a separate checkout — a *worktree* — for each piece of agent work, and this is where those go. Keep the default unless you need them on another local volume or in a directory your organization requires.

   Whichever you choose, it needs room for several complete checkouts of your project.

   Your own working directory is never touched by agent work. That isolation is the point of the worktree, and it is worth knowing before an agent starts editing.

7. Click **Create Project**.

   The button reads **Creating...** while RalphX works. Leave the app open until it finishes.

   Use **Cancel** only if you opened this dialog outside the first-run flow and want to leave without creating anything.

8. Click **Continue** when the **Project** step reports “Project workspace ready.”

## Optionally connect Atlassian

1. Click the **Atlassian** step to connect Jira and Confluence now, or skip it.

   This step is optional and nothing later depends on it. It adds Jira and Confluence context to agent conversations.

   You can connect it any time afterwards through Settings → **Integrations** → **Atlassian**, covered in [Connecting RalphX to Jira and Confluence](integrations/connect-jira-and-confluence.md).

## What you have now

RalphX is installed on your Mac with an authenticated agent provider available. You have a project with a base branch set and a worktree parent directory chosen, so agent work has somewhere isolated to happen. The optional Atlassian connection is either configured or safely left for later.

## Next

- [Finding your way around](02-tour-of-the-app.md)
- [RalphX concepts](concepts.md) — if *worktree*, *harness*, or *provider* did more work above than you expected
