# Agent Personas GA Gates

Agent Personas remain behind `RALPHX_UI_AGENT_PERSONAS=1` until the two empirical gates below pass and their results are recorded here.

## Gate Status

| Gate | Status | Result / evidence |
|---|---|---|
| Claude `--resume` + appended-prompt smoke | PENDING | — |
| Packaged-app TCC/protected-folder ingestion smoke | PENDING | — |
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

## Gate 2: Packaged-App TCC / Protected-Folder Ingestion Smoke

**Status:** PENDING

1. Run `npm run tauri build`, then launch the packaged app from Finder.
2. In **Settings → Personas → Build with agent**, pick a folder under `~/Documents` and a folder that contains an unreadable subdirectory.
3. Confirm macOS asks for permission only at pick time, never from the child agent.
4. Confirm the manifest reports copied files and any `EPERM` / cap-skipped entries without blocking preview or chat.
5. Spot-check the extractor debug log: `RALPHX_FILESYSTEM_READ_ROOTS` must contain only ingestion-store copies.

| Run date | Packaged build | Result | Manifest / debug-log evidence |
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
