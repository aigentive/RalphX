# Codex Agent Approvals And Security

Official docs: `https://developers.openai.com/codex/agent-approvals-security`

Snapshot notes:

- Codex docs separate approvals/security from generic sandboxing.
- Official config reference ties approvals to `approval_policy` and granular approval controls.
- Protected paths, network access, and sandbox/approval interactions are first-class topics.

RalphX notes:

- RalphX must map its execution halt and operator expectations onto Codex approval semantics without assuming Claude behavior.
- Provider/lane settings and compatibility-locked workflows stay on `approval_policy="never"`.
- Standalone Chat is a narrow backend-owned exception: RalphX emits `approval_policy="on-request"` together with the contained workspace sandbox, while Project and Standalone PersonaBuilder launches retain the compatibility policy.
- RalphX launches Standalone Chat through non-interactive `codex exec`; an action that requires a fresh approval cannot open an interactive prompt, so Codex fails that action closed and surfaces the error.
