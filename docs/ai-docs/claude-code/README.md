# Claude Code Integration Notes

RalphX currently integrates with the Claude CLI, but Claude Code itself is an external dependency and not the long-term only CLI target for this repo.

This directory is intentionally lightweight:

- Official Claude Code docs are the source of truth.
- Topic stubs in this folder exist so agents can grep-discover the relevant Claude Code surface area from inside the repo.
- RalphX-specific behavior that is load-bearing stays local here.

Use the official docs when current vendor behavior matters:

- Docs index: `https://code.claude.com/docs/llms.txt`
- Main docs root: `https://docs.anthropic.com/en/docs/claude-code`

Local files in this directory:

- Topic stubs such as `cli-reference.md`, `settings.md`, `mcp.md`, `agent-teams.md`
- RalphX-specific note: `task-tool-parallel-dispatch.md`
- RalphX-specific [model compatibility](model-compatibility.md)
