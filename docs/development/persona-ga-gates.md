# Agent Personas GA Gates

Agent Personas remain behind `RALPHX_UI_AGENT_PERSONAS=1` until the two empirical gates below pass and their results are recorded here.

## Gate Status

| Gate | Status | Result / evidence |
|---|---|---|
| Claude `--resume` + appended-prompt smoke | PENDING | — |
| Packaged-app TCC/builder-context smoke | PENDING | — |
| ARG_MAX headroom measurement | PENDING | — |

## Gate 1: Claude `--resume` + Appended-Prompt Smoke

**Status:** PENDING

Run against a live development build with `RALPHX_UI_AGENT_PERSONAS=1`.

1. Create an Agents-workspace conversation, bind persona A with a distinct voice marker, and send a message.
2. Use `scripts/find-debug-logs.sh` to confirm the spawn includes `<ralphx_agent_persona>`.
3. Switch the conversation to persona B and confirm stop-on-switch runs. Send a second message.
4. Confirm the next spawn contains both `--resume <provider_session_id>` and persona B's appended block. Confirm the reply reflects persona B and correctly references earlier conversation history.

If any check fails, enable the A20 fallback (`persona_switch_forces_fresh_provider_session`) and record the failure, fallback state, debug-log location, and observed reply here.

| Run date | Build / harness | Persona A → B | Result | Evidence / notes |
|---|---|---|---|---|
| — | — | — | PENDING | — |

## Gate 2: Packaged-App TCC / Builder-Context Smoke

**Status:** PENDING

1. Run `npm run tauri build`, then launch the packaged app from Finder.
2. Start a project-scoped **Build with Agent** flow and confirm RalphX opens the standard **Agents** view with **Persona** mode locked.
3. In the Agents composer, attach a UTF-8 text file under `~/Documents` and add a folder reference that contains an unreadable subdirectory.
4. Confirm macOS requests protected-folder access from the packaged app, the text attachment is materialized into the private builder workspace, and the folder remains a live reference.
5. Spot-check the builder debug log: `RALPHX_FILESYSTEM_READ_ROOTS` must contain only the private workspace, the selected project directory, and the attached folder roots. Confirm the agent cannot read outside those roots and access to the unreadable subdirectory fails closed without blocking the conversation.

| Run date | Packaged build | Result | Workspace / debug-log evidence |
|---|---|---|---|
| — | — | PENDING | — |

## Gate 3: ARG_MAX Headroom Measurement

**Status:** PENDING (measurement)

Record the worst-case appended prompt at approximately 80 KB (persona, skills, and profile) against macOS `ARG_MAX` of approximately 1 MB: about 8% headroom use. This confirms A19 tempfile delivery is prompt-transport hygiene, not a GA blocker.

| Run date | Worst-case prompt | ARG_MAX | Headroom use | Result / notes |
|---|---|---|---|---|
| — | ~80 KB | ~1 MB | ~8% | PENDING |

## Flag-Flip Criteria

Before enabling Agent Personas by default, record each item as complete:

- [ ] The propagation-absence matrix cross-check maps every matrix ID to a landed, passing test.
- [ ] A dogfood period completed with `RALPHX_UI_AGENT_PERSONAS=1`.
- [ ] Gate 1 and Gate 2 are recorded as PASS above.
- [ ] The Phase 4 design attestation is recorded.
