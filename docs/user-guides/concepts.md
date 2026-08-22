# RalphX concepts

Seven words appear over and over in these guides and in the app itself. This page defines each one in plain language and points at the guide that puts it to work. Read it once before you start, or come back to it whenever a term in another guide is doing more work than you expected.

**Before you start:** nothing — you can read this before installing anything.

## Base branch

The branch your work is meant to merge into. On most repositories that is `main`.

RalphX asks for it when you create a project, and stores it in Settings → **Repository** → **Repository** under **Base Branch**, whose helper text reads “The branch tasks are merged into.”

Everything an agent does starts from a fresh copy of this branch and is meant to come back to it. Point it at your normal integration branch, not at a feature branch.

Set up in [Teaching RalphX about your project](configure/project-setup-and-validation.md).

## Worktree

A separate checkout of your repository on its own branch, created by RalphX for one piece of work.

This is the single most important thing to understand about how RalphX behaves: **agents never edit your working directory.** When you start work, RalphX creates a worktree from the base branch, and the agent edits files there. Your own checkout keeps whatever you had open, uncommitted changes included.

Worktrees live under the directory you set as **Worktree Location** — “Directory where task worktrees are created.” RalphX uses `~/ralphx-worktrees` if you leave it unset.

A worktree is a real git worktree, so you can open it in your editor, run its tests, and inspect its branch like any other checkout.

## Harness

The AI runtime RalphX drives — currently Claude Code or Codex.

RalphX is not a model. It launches a coding-agent CLI you have already installed and authenticated, supervises it, resumes it, and interprets the events it streams back. The harness is that CLI. RalphX stores no API credential of its own; it reuses the login already in your `claude` or `codex` CLI.

Because the harness is external, its capabilities are its own. Which models you can pick, and whether a feature such as Codex Fast is available, depends on the version of the CLI you have installed — not on a RalphX setting.

## Provider

A harness that you have enabled in RalphX, together with its settings.

Settings → **Models & Providers** → **Providers** shows one card per harness. Each card carries two independent pieces of status:

- **Enabled**, **Ready**, or **Not ready** — whether you have switched this provider on.
- **CLI Ready** or **CLI Not Ready** — whether RalphX can actually find and run the CLI on your machine.

A provider can be **Enabled** and still **CLI Not Ready**, which means you switched it on but the CLI is missing or not working. That pairing is the usual cause of a run that will not start.

Once a provider is enabled, Settings → **Agents** → **Roles** decides which provider and model each agent role uses.

Set up in [Using Claude Code with RalphX](configure/using-claude-code-with-ralphx.md) or [Using Codex with RalphX](configure/using-codex-with-ralphx.md).

## Plan bundle

The pair of documents RalphX produces in **Plan** mode: a **Plan Overview** and an **Implementation Blueprint**.

The Overview is the one you read and argue with — goal, decisions, risks, what is in and out of scope. The Blueprint is the one the implementing agent reads — exact files, exact steps, exact things to prove.

They are approved together, which is why the app calls them a bundle. Approving the plan is what unlocks **Implement Directly** and **Create Proposals**.

Covered in [Planning a feature with RalphX](workflows/planning-a-feature.md).

## Artifact

Anything an agent produces that is a document rather than a message.

The right-hand pane of an agent conversation holds them, one tab each: **Issues**, **Plan**, **Tasks**, **Review**, and **Commit & Publish**. Which tabs appear depends on what the conversation has produced so far.

The distinction worth holding on to is that the left side is the conversation — a transcript that scrolls away — and the right side is the durable result. When a guide tells you to look at the plan or the review findings, it means the right-hand pane.

## Workspace Review

A local quality gate: RalphX reviews the changes in a worktree before they leave your machine.

You start it with **Run review** on the **Review** tab. It reads the diff, reports findings, and can be told to fix them. Nothing is sent anywhere while this happens.

**This is not a GitHub review.** No pull request is involved, no comment is posted, and nobody else sees it. It is a check you run on your own work before publishing. Reviewing an actual pull request on GitHub is a different mode entirely.

Covered in [Reviewing your own work with RalphX](workflows/reviewing-your-own-work.md), and contrasted with the GitHub kind in [Reviewing a pull request with RalphX](workflows/reviewing-a-pull-request.md).

## Which harness should I use?

You need exactly one to start, and you can change your mind later — the choice is per agent role, not per app, so you can move one lane at a time.

| | Claude Code | Codex |
|---|---|---|
| Install | `claude` CLI, authenticated | `codex` CLI, authenticated |
| Controls you will set | Permission mode | Approval policy **and** sandbox mode |
| Worth knowing | The broadest current feature coverage in RalphX | RalphX's MCP tools need Codex's most permissive approval and sandbox settings; hardening them turns those tools off |
| Guide | [Using Claude Code with RalphX](configure/using-claude-code-with-ralphx.md) | [Using Codex with RalphX](configure/using-codex-with-ralphx.md) |

If you have no strong preference and are just getting started, install whichever CLI you already have an account for. Neither choice is hard to reverse.

## What you have now

You can read the rest of the guides without stopping to work out what a worktree, a harness, or a plan bundle is. The two ideas that carry the most weight are that agents work in an isolated worktree rather than your checkout, and that Workspace Review is local and private rather than a GitHub review.

## Next

- [Installing RalphX and running it for the first time](01-install-and-first-run.md)
- [Finding your way around](02-tour-of-the-app.md)
