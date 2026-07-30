# Codex Stream Fixtures

Captured live provider output. Do not hand-edit — recapture instead.

| Fixture | CLI | Captured |
|---|---|---|
| `exec_json_reasoning_0_146_0.jsonl` | `codex-cli 0.146.0` | 2026-07-30 |

## Recapture

```bash
mkdir -p /tmp/codex-reasoning-probe && cd /tmp/codex-reasoning-probe
printf 'alpha\nbeta\ngamma\n' > a.txt && printf 'one\ntwo\n' > b.txt
printf 'Inspect every file in this directory with shell commands, then tell me the total number of lines across all files and which file has the most lines. Verify by running at least two different commands.' \
  | codex exec --json -s read-only --skip-git-repo-check \
      -c 'model_reasoning_effort="high"' -c 'model_reasoning_summary="concise"' -
```

The two `-c` overrides mirror what `build_codex_exec_args_with_security_policy` injects. Without
`model_reasoning_summary`, the same prompt produced zero `reasoning` items — the flag is what makes
Codex reasoning observable at all.

## Confirmed schema (`codex exec --json`, 0.146.0)

Events are `thread.started` | `turn.started` | `turn.completed` | `turn.failed` | `item.started` |
`item.updated` | `item.completed`. Item types are `agent_message` | `reasoning` |
`command_execution` | `file_change` | `mcp_tool_call` | `web_search` | `todo_list`.

Reasoning arrives as `item.completed` with `item.type == "reasoning"` and a flat `text` field
holding the summary parts joined by `\n`. There is no `summary` array and no `event_msg` envelope on
this transport.

`event_msg` belongs to the persisted session rollout (`~/.codex/sessions/**/rollout-*.jsonl`), which
RalphX never reads. There the envelope key is `payload`, reasoning is tagged `agent_reasoning`, and
the `summary: [{type: "summary_text", text}]` array appears on `response_item` payloads.
