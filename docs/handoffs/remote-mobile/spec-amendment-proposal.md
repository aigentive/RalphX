# Remote Multi-Environment — Spec Amendment Proposal (post-implementation alignment, 2026-07-29)

Source: 5-lens Opus spec-alignment review + Fable assessment over integration `19078c578` (34 findings vs 109 confirmed obligations; 20 code fixes landed in `fix/rme-spec-alignment`). This document carries the SPEC-side output: amendments the owner should fold into the gist spec, deviations to record, and resolved decisions.

## Amendments (code is right; spec text should absorb)

### A1. §6.3 (source-spec.md:608), Phase-0 acceptance (01-phase-0-foundations.md:123,132,161), cross-spec contract #8 (source-spec.md:878), §6.5 blocked list (source-spec.md:628) + 03-phase-2 PR 2.3 key point 10

The canonical error taxonomy is TEN codes: the original eight plus REMOTE_INVALID_ARGUMENTS (a 400 argument-shape rejection, distinct from 404 command-unavailable; classified non-retryable — an identical resend cannot succeed) and REMOTE_INTERNAL_ERROR (host-side 500, distinct from transport failure). Update §6.3's list, un-check and re-word the Phase-0 'exactly eight, none extra' acceptance to 'exactly ten', update cross-spec contract #8 (the mobile spec consumes this list — R5-L1 already demonstrated the stale-list failure mode once), and widen §6.5's blocked-entry list from three to four causes: 401/403, version mismatch, malformed descriptor, invalid arguments.

**Rationale:** The 8→10 split is implemented coherently on both sides with sound in-code reasoning (lib.rs:167-174, transport-errors.ts:26-33) and real producers/consumers; the code is right and the spec is the stale artifact. This absorbs three lens findings (taxonomy size, §6.5 fourth blocked cause, cross-spec export) into one amendment.

### A2. §3.1 endpoint table (source-spec.md:141), §4.2 sequence diagram (:403), 03-phase-2-client-mode.md:198

The WS event-stream endpoint is GET /remote/v1/events?ticket=… (not /remote/v1/ws). Update all three occurrences.

**Rationale:** Host and client shipped /remote/v1/events consistently (ws.rs:81, remote_ws_client.rs:31). §3.1 is explicitly framed as the cross-spec/mobile contract, so the published path must match the wire; renaming the code back would be pointless churn.

### A3. §3.1 endpoint table + §6.1 remove flow (source-spec.md)

Add POST /remote/v1/auth/revoke to the endpoint table: bearer-authed, self-scoped (identity resolved by the middleware, no device-id argument), durable-revocation-first ordering, 500 on repo failure. It exists to satisfy §6.1's remove flow ('host revoke (best-effort) → Keychain delete → row delete').

**Rationale:** A security-surface endpoint the §6.1 flow requires but the §3.1 contract never listed. The implementation (auth_endpoints.rs:259-307) is sound; the contract table must enumerate every mounted route or the mobile spec cannot trust it as exhaustive.

### A4. §6.4 warm-up clause + 03-phase-2-client-mode.md PR 2.4 key point 7 / ordered task / C-8

Drop the onPointerEnter descriptor-probe warm-up for the environment switcher. Replace with: 'The switcher is a pure store consumer; rendering or hovering it must not touch remote lifecycle (P-14). C-8 covers first-paint synchronous switching only.' If the owner wants a hover warm-up later, it must be redesigned to be provably P-14-compatible (no descriptor fetch attributable to an inactive environment).

**Rationale:** The implementation discovered a genuine conflict the six rounds missed: a hover-triggered descriptor probe against a non-active environment contradicts P-14/P-26's active-env-only proxy authorization (authorize_proxy_target rejects non-active invoke targets). The pr24-c handoff contract reversed the obligation implicitly (handoffs/pr24-c-contract.md:59) — this makes the reversal explicit and owner-visible.

### A5. P-17b (source-spec.md:717) and C-13b (:720) member lists

Remove add_task_note, send_teammate_message, send_team_message from the P-17b/C-13b required-member lists. send_teammate_message/send_team_message: the Team feature was removed from main after the spec pin (already recorded at 02-phase-1-host-mode.md:169). add_task_note: it is an HTTP handler on :3847, not a Tauri command, so it can never appear in the census-derived suite; its safety property is discharged instead by P-1's remount denial (fetch_remount_tests.rs:276 asserts /api/add_task_note stays unmounted on :3849) and by its presence on the content-writer surface as an http-handler writer.

**Rationale:** Three spec-named proof anchors are structurally unsatisfiable; leaving them in the spec invites a future reader to either treat them as a coverage gap or re-add them as anchors and wedge the suite. The substitution mechanism is real and tested — it just needs to be written down.

### A6. Drift control #1 manifest schema (source-spec.md:284)

The shipped ledger/facade row shapes omit `scope` and `argNames` from the declared [{cmd, riskClass, capabilities, scope, argNames, …}] schema. Amend to the shipped shape with two notes: scope is mechanically derivable from riskClass via scope_for_class (registry.rs:85-93) and must not be duplicated; the wire arg surface is guarded by drift-control item 2 (the P-11 AST scan) rather than by manifest argNames. If a manifest consumer ever needs argNames (e.g. the mobile client), that is a schema addition to request from the owner, not silent drift.

**Rationale:** The reduction is recoverable (scope) or compensated (argNames via P-11), and re-plumbing argNames now would add generator surface with no current consumer. Documenting the reduced schema keeps the manifest's authority claim honest.

## Deviations to record (sound; need only documentation)

### D1. §3.4 durable globs narrowed to UI-consumed ∪ explicitly-enumerated names

**Where:** .artifacts/specs/remote-multi-env/tracker.md (deviations section) + un-check/re-word 01-phase-0-foundations.md:121 key point 8 and the :156-166 acceptance line

DEVIATION (sound): the event classification table does not expand the §3.4 globs (task:*, team:*, automation:*, ticketing:*) in full; it seeds UI-consumed ∪ explicitly-chosen names and publishes the residue as unclassified_backend_emits (48 names, e.g. team:artifact_created, ticketing:operation_updated, task:merged). Rationale lives in scripts/event-manifest-scanner/src/lib.rs:216-223. Safety: consumed ⊆ classified is CI-enforced and consumed ∩ unclassified is currently empty, so any FUTURE frontend consumer of an unclassified durable name fails CI at regenerate-and-diff rather than silently losing events remotely. The Phase-0 'globs are expanded to enumerated names' acceptance is therefore reworded, not satisfied as originally written.

### D2. run_task_validation classified NonContent despite R6-L2 naming it a reviewer content tool

**Where:** tracker.md deviations + a one-line comment at the NON_CONTENT_TOOLS entry (capability_ledger_tests.rs:156)

DEVIATION (sound): R6-L2 lists run_task_validation alongside get_task_validation_summary as reviewer-readable content. Implementation classifies run_task_validation NonContent because it EXECUTES validation rather than returning persisted content; its output is reachable only through get_task_validation_summary, which IS on the enumerated content-read surface. Content-surface obligations therefore attach to the read tool, not the trigger.

### D3. create_project/update_project ledgered Elevated while their spec-sentence siblings are Denied, and unpinned

**Where:** tracker.md deviations

DEVIATION (compliant but unpinned): §3.3 backstop #1 names create_project/update_project ('git-init at arbitrary caller paths') in the same extended-deny sentence as setup_gh_git_auth et al. The four auth/credential siblings are Denied and pinned by P-17c; create_project/update_project are Elevated ['spawnsProcess'] — permitted by P-17c's literal 'Denied/Elevated' — but appear in no pin set, so nothing fails if they later drop to agentControl. Recommended follow-up (can ride any ledger PR): add both to a pinned Elevated-or-stronger floor in the P-17c test.

### D4. Background environments adopt the pair-time scopes snapshot into the confirmed-scope slot

**Where:** tracker.md (extend the existing orphaned-scope ledger line for 3.3-a)

DEVIATION (bounded, interim): for non-active environments, refreshScopes returns getConfirmedScopes ?? pairing snapshot without a GET /remote/v1/session call, and the supervisor writes that set through applyScopes — the path three in-repo contracts (environmentStore.ts:111-116, agent-gate.ts:216-218, §6.6/P-28) document as CONFIRMED-only. Deliberate (asserted by environment-runtime.test.ts:476-490) and currently safe: useEnvironmentWritable fails closed unless presentation === connected, and activate() restarts the supervisor (full introspection) on switch, so the unconfirmed set never backs an enabled affordance. 3.3-a (which owns background scope refresh) MUST replace this with real introspection or a distinct 'provisional' slot rather than widening the confirmed-slot semantics further.

## Additional structural amendment (from the chat-send lane, post-lens)

### A7. P-17b membership generation (§3.3)

P-17b membership is generated from detector output, so a command that is detector-silent BY DESIGN (the spawn-free steering pattern, e.g. `send_remote_chat_message`) is invisible to the generated negative suite. Amend the P-17b contract: every spawn-free steering command MUST carry an explicit `DECLARED_MEMBERSHIPS` row (reason-coded, e.g. `steering-persisted-turn`), and the strip-membership visibility test covers it. This generalizes the R4-C3 hybrid rule to the registration era.

## Owner decisions resolved under delegation (2026-07-29, recommendation adopted)

### R1. MIN_CLIENT_PROTOCOL aliased to PROTOCOL_VERSION — hard cutover vs additive evolution

**Question:** src-tauri/src/remote_server/endpoints.rs:26 defines the host's client floor as `= PROTOCOL_VERSION`, so the day PROTOCOL_VERSION becomes 2 every shipped v1 client (including the future mobile app) is refused at the descriptor gate — the opposite of R-7's additive-only evolution (source-spec.md:797). The comment at the definition argues for an independently tunable floor, but the value makes tightening automatic. Which acceptance policy do you want: floor rides the version (hard cutover), or floor pinned independently at 1 until a deliberate raise?

**Resolved:** Pin MIN_CLIENT_PROTOCOL as an independent constant at 1 and add a code comment + spec note that raising it is a deliberate compatibility decision, never a side effect of bumping PROTOCOL_VERSION. Inert today (both are 1), so this is a one-line change plus policy text — cheapest now, before the mobile spec ships a client against it.

### R2. Pairing-card QR code: implement, or formally drop from the spec

**Question:** §4.2 and §5.4 both name a QR affordance, and tracker.md's R-12 decision was 'preferred-endpoint-only QR' — i.e. QR was decided, not dropped — yet RemotePairingCard.tsx:159-161 ships only the grouped code + copyable URL, with an inline comment deferring QR because no QR library exists in frontend/package.json. Add the dependency (or a small embedded encoder) to implement it, or amend §4.2/§5.4 and the R-12 record to drop QR?

**Resolved:** Implement it. The URL-encoding plumbing already exists (remote-access-utils.ts pairingUrl), the mobile client's primary pairing gesture per §4.2 is scanning, and a dependency-free QR encoder is ~200 lines or one tiny audited package. Dropping it would degrade the flagship mobile pairing flow the spec was written for.

### R3. P-4 parity scope: extend tables to mutating ops, or amend the proof to Read-only

**Question:** All four p4_parity_* tables cover Read commands; the 27 registered Operate/AgentControl ops have no result-vs-direct-call envelope parity assertion (their tests assert authorization and absence-of-effect instead). Mutating parity requires paired fixture executions and is meaningfully more test machinery. Extend parity to mutating ops, or amend P-4 to record the Read-full / mutating-representative scoping?

**Resolved:** Middle path: add one parity table over a representative mutating trio (create_task, update_task, deny_permission_request) including error-path envelope parity, and amend P-4 to state Read = exhaustive, mutating = representative + scope-suite effect coverage. Full mutating enumeration is low marginal value given the scope suite already proves effects and refusals per op.
