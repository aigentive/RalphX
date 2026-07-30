# RalphX User Guides

RalphX is a native Mac app that runs coding agents against your own git repositories. You describe what you want, RalphX plans it with you, implements it in an isolated git worktree, reviews the result, and opens a pull request — with your approval at each gate. Your repository, your branches, your review; RalphX is the workflow layer around them.

These guides are about **using** the app. They do not cover building RalphX from source — that lives in [`docs/development/build-from-source.md`](../development/build-from-source.md).

## Start here

**New to RalphX? → [Installing RalphX and running it for the first time](01-install-and-first-run.md)**

Then read [Finding your way around](02-tour-of-the-app.md), and follow the workflow guides in order. Together they take you from a fresh install to a merged change.

## I want to…

| I want to… | Guide |
|---|---|
| Install RalphX and get it running | [Installing RalphX and running it for the first time](01-install-and-first-run.md) |
| Understand what I'm looking at | [Finding your way around](02-tour-of-the-app.md) |
| Plan a feature before building it | [Planning a feature with RalphX](workflows/planning-a-feature.md) |
| Build the thing I just planned | [Implementing a feature with RalphX](workflows/implementing-a-feature.md) |
| Check my changes before publishing them | [Reviewing your own work with RalphX](workflows/reviewing-your-own-work.md) |
| Review a GitHub pull request | [Reviewing a pull request with RalphX](workflows/reviewing-a-pull-request.md) |
| Ask questions about a codebase without changing it | [Asking questions about a codebase](workflows/asking-about-a-codebase.md) |
| Connect RalphX to GitHub | [Connecting RalphX to GitHub](integrations/connect-github.md) |
| Choose and configure my agent runtime | [Using Claude Code with RalphX](configure/using-claude-code-with-ralphx.md) · [Using Codex with RalphX](configure/using-codex-with-ralphx.md) |
| Teach RalphX how to build and check my project | [Teaching RalphX about your project](configure/project-setup-and-validation.md) |
| Work from Jira, Linear, or ClickUp tickets | [Working with tickets](integrations/ticketing-dashboard.md) |
| Fix something that went wrong | [When something goes wrong](troubleshooting.md) |

## The main path

The five workflow guides follow one another. Each states its prerequisite and links the next.

```
01 Install  →  02 Tour  →  Planning  →  Implementing  →  Reviewing your own work
                                                                  │
                              Connect GitHub  →  Reviewing a pull request
```

## Connecting your other tools

Each connect guide stands alone. Read only the one for the tool you are connecting.

| Tool | Guide |
|---|---|
| GitHub | [Connecting RalphX to GitHub](integrations/connect-github.md) |
| Jira and Confluence | [Connecting RalphX to Jira and Confluence](integrations/connect-jira-and-confluence.md) |
| Jira and Confluence, once connected | [Using Jira and Confluence in RalphX](integrations/using-jira-and-confluence.md) |
| Linear | [Connecting RalphX to Linear](integrations/connect-linear.md) |
| ClickUp | [Connecting RalphX to ClickUp](integrations/connect-clickup.md) |
| Granola | [Connecting RalphX to Granola](integrations/connect-granola.md) |
| Tickets from any connected tracker | [Working with tickets](integrations/ticketing-dashboard.md) |

## Configuring RalphX

| Topic | Guide |
|---|---|
| Using Claude Code with RalphX | [Using Claude Code with RalphX](configure/using-claude-code-with-ralphx.md) |
| Using Codex with RalphX | [Using Codex with RalphX](configure/using-codex-with-ralphx.md) |
| Teaching RalphX about your project | [Teaching RalphX about your project](configure/project-setup-and-validation.md) |
| Controlling how much runs at once | [Controlling how much runs at once](configure/capacity-and-concurrency.md) |

## Going further

Some RalphX features are **off by default** and stay off until you turn them on in Settings. They are not part of the main path above, and you can ignore them entirely until you want them.

| Topic | Guide |
|---|---|
| Tracked delivery with Tasks — includes the Kanban board and dependency graph, off by default | [Tracked delivery with Tasks](advanced/task-managed-delivery.md) |
| Delivering large projects with automated, supervised goals | [Delivering large projects with automated, supervised goals](advanced/delivering-large-projects.md) |
| Customizing your agents' voice with Personas — off by default | [Customizing your agents' voice with Personas](advanced/personas.md) |
| Connecting other tools to RalphX | [Connecting other tools to RalphX](advanced/external-access.md) |
| When something goes wrong | [When something goes wrong](troubleshooting.md) |

## Looking for something else?

- **Building RalphX from source** — [`docs/development/build-from-source.md`](../development/build-from-source.md)
- **Installing, upgrading, repairing, uninstalling** — [`docs/install/homebrew.md`](../install/homebrew.md)
- **How RalphX works internally** — [`docs/architecture/`](../architecture/)
- **Driving RalphX from another tool** — [`docs/external-mcp/`](../external-mcp/)
