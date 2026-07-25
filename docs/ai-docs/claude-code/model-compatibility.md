# Claude model compatibility

RalphX recognizes these active pinned Claude Code model IDs only when the installed CLI has reached their capability floor:

| Exact model ID | Status | Minimum Claude Code |
| --- | --- | --- |
| `claude-opus-4-7` | Active | `2.1.111` |
| `claude-opus-4-8` | Active | `2.1.154` |
| `claude-opus-5` | Active | `2.1.219` |

Native aliases `sonnet`, `opus`, `haiku`, and `fable` are provider-owned values. RalphX stores and launches them byte-for-byte; it does not translate aliases to pinned model IDs. Pinned IDs are validated against the discovered CLI capability: a launch below its floor fails closed, while refreshing capabilities re-probes the installed CLI.

Vendor references:

- [Claude Code changelog (raw)](https://raw.githubusercontent.com/anthropics/claude-code/refs/heads/main/CHANGELOG.md)
- [Anthropic model IDs and versioning](https://platform.claude.com/docs/en/about-claude/models/model-ids-and-versions)
- [Anthropic model deprecations](https://platform.claude.com/docs/en/about-claude/model-deprecations)
