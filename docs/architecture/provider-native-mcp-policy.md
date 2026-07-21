# Provider-Native MCP Policy

RalphX-launched Claude and Codex agents inherit third-party MCP servers from the provider's native configuration. RalphX owns only its required internal server injection and a deny-capable policy overlay; it never copies or writes provider server definitions, commands, URLs, environment, headers, credentials, approvals, authentication, or trust state.

## Resolution

Server identity is `(provider, server_id)`. Server and tool fields resolve independently in this order:

1. locked RalphX internal requirement;
2. project UI override;
3. project `.ralphx/mcp.yaml`;
4. global UI override;
5. global `~/.ralphx/mcp.yaml`;
6. provider-native state.

Overrides are `follow`, `enabled`, or `disabled`. `enabled` removes a lower RalphX deny but cannot activate a native disabled, absent, unapproved, unauthenticated, or untrusted server. Policies remain stored while a provider is disabled or unavailable.

Invalid YAML is diagnostic and blocks launch rather than being treated as an empty policy. Clearing a server override returns that field to `follow` without deleting independent tool overrides.

## Launch Enforcement

- Claude receives the generated required RalphX `--mcp-config` without `--strict-mcp-config`; server/tool denies become `--disallowedTools` MCP patterns.
- Codex keeps native config layers; denies quote arbitrary server-ID key segments such as `mcp_servers."<id>"`. RalphX servers are `enabled=true`, `required=true`, and use `external_mcp.startup_timeout_secs` for startup/readiness.
- A provider-native server using reserved ID `ralphx` or `ralphx_internal` fails launch preflight instead of being overwritten, except for exact Claude user scope + server ID `ralphx`. That identifier is reserved RalphX state regardless of its definition: startup and launch preflight serialize revalidation, remove it through the resolved Claude CLI, pin the child `HOME` to the same validated config root used for discovery, and rediscover provider state after every possibly-started removal command. Verified absence continues the initiating launch even after a non-zero exit or terminated timeout; continued presence or an indeterminate read fails closed. Project/local entries, Codex entries, near names, and `ralphx_internal` remain untouched.
- Every standard, interactive, queued-resume, recovery, utility, and teammate spawn resolves policy immediately before launch.
- New Agent conversations additionally run MCP setup preflight before worktree, conversation, workspace, event, or run side effects. Startup reconciliation is best-effort and runs even when unrelated startup recovery is disabled; the immediately-before-launch seam remains authoritative.
- Agents using the external RalphX transport wait for supervisor `Ready`; Disabled, Degraded, Failed, and timeout states fail closed.

## Management Surface

Harness → MCP is available only for providers whose persisted setting is enabled and whose refreshed runtime probe is available. It provides global/project server and tool overrides, exact-name denies when tool discovery is limited, redacted provider diagnostics, and the existing global RalphX external bridge controls. Native provider configuration remains the place to add servers, authenticate, approve project MCP files, or establish trust.

Catalog and mutation commands recheck provider readiness. Codex catalog discovery uses the effective provider `CODEX_HOME` and structured app-server `config/read` plus paginated `mcpServerStatus/list`; unsupported versions fall back to fixed native config paths with a visible diagnostic. Catalog payloads contain only provider/server/tool identifiers, status, policy provenance, repair classification, and bounded diagnostics. Raw commands, arguments, environment, headers, credentials, URLs, and provider definitions never cross the backend boundary.

Managed retry remains in-app. Agent start snapshots the failed scalar/reference input plus live attachment `File` objects and folders before cleanup, gates cleanup and Send under one busy state, and reuses the normal submit path exactly once after verified absence. Settings retry refreshes the catalog only. Neither surface displays terminal remediation.
