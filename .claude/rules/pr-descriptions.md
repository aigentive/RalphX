> **Maintainer note:** Keep this file compact. PR bodies are for reviewers and users; CI is for command logs.

# PR Description Rules

## Source Of Truth

| Rule | Detail |
|---|---|
| Template first | Follow `.github/PULL_REQUEST_TEMPLATE.md`; it is the shared format for humans and agents |
| Missing template | For external projects without a PR template, use RalphX's fallback template with the same section intent |

## Rules

| Rule | Detail |
|---|---|
| Impact first | Lead with context, user-facing changes, and why the change matters |
| Validation secondary | Do not dump command transcripts; CI is the source of truth for routine validation |
| Manual evidence only | Mention manual/visual validation only when it adds review value beyond CI |
| No agent diary | Omit implementation chronology, "I ran...", and raw local terminal output |
| Explicit scope | State meaningful non-goals or deferred work when reviewers might expect them |
