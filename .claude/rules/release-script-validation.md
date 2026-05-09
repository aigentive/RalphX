---
paths:
  - "scripts/release.sh"
  - "scripts/propose-release.sh"
  - "scripts/generate-release-notes.sh"
  - "scripts/append-github-release-metadata.sh"
  - "scripts/bump-version.sh"
  - "scripts/release-analysis-common.sh"
  - "scripts/prompts/release-*.md"
  - "docs/release-process.md"
  - "release-notes/README.md"
---

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

# Release Script Validation

## Rules

| # | Rule |
|---|------|
| 1 | **Main checkout validation stays local-only** — use `bash -n`, `--help`, `./scripts/propose-release.sh --context-only`, `./scripts/generate-release-notes.sh --context-only --output <tmp>`, and `./scripts/append-github-release-metadata.sh --strip-only --notes-file <file> --output <tmp>` in the real repo. |
| 2 | **Wrapper e2e runs use a disposable stub repo** — validate `./scripts/release.sh` end-to-end only in a temp repo/worktree with a fake `codex` binary and stubbed release scripts. ❌ Full wrapper run in the main checkout. |
| 3 | **No publish side effects during validation** — stop before tag creation, `git push`, workflow dispatch, Homebrew/update publication, or any other release action. |
| 4 | **Stored version state is test input** — `.artifacts/release-notes/.version` is local/gitignored, affects no-arg `bump-version.sh` and `generate-release-notes.sh`, and must never be committed. |
| 5 | **Interface changes require doc sync** — if release script flags, prompts, or artifact paths change, update `docs/release-process.md` and `release-notes/README.md` in the same slice. |
