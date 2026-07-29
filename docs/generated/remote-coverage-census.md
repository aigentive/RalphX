# PR 3.1 — Facade coverage census (P-11 gap work manifest)

> GENERATED — do not edit by hand. Regenerate: `node scripts/generate-remote-coverage-census.mjs`. Staleness gate: `--check`.
> This is the PR 3.1-a planning artifact. It registers nothing. Every class here is the ledger's CURRENT value; the per-command hand audit (§3.3) and the P-17 detector run own the final one.

## 1. Scan state

```
PASS: remote transport drift — 499 invoke command name(s), 0 dynamic, 0 seam bypasses; 248 manifest-classified; 48 unclassified (baseline, → 0 in PR 3.1).
```

| Measure | Count | Source |
|---|---|---|
| Invoke command names in `frontend/src` | 499 | drift scan (AST) |
| Dynamic / unresolvable expressions | 0 | drift scan — must stay 0 |
| Transport seam bypasses | 0 | drift scan — must stay 0 |
| Remote-registered (`remote_commands!`) | 208 | `docs/generated/remote-commands.json` |
| Reason-coded local-only rows | 29 | `frontend/src/lib/remote/local-only-commands.ts` |
| Ledger rows (exhaustive over `generate_handler!`) | 546 | `docs/generated/remote-commands.json` |
| Manifest-classified (host-denied / v1-deferred) | 248 | `v1Resolution` in `docs/generated/remote-commands.json` |
| **Unclassified — the 3.1 gap** | **48** | `scripts/remote-transport-drift-baseline.json` |

## 2. What the gap is made of

Routing each name mechanically through the ledger splits it into very different kinds of work. B0 has already retired the three non-registerable dispositions from the gap, so they read 0 here — their members now resolve through the manifest and no longer sit in the baseline:

| Disposition | Count | Rule |
|---|---|---|
| register-candidate | 48 | ledgered AgentControl (or lower) with no SpawnsProcess capability — eligible for a hand-audited `remote_commands!` entry under `ui:agent` |
| host-denied (class: denied) | 0 | `class_permits` returns false for Denied at any capability set — registering it fails compilation. Resolves for P-11 through the manifest, never through a local-only reason (phase doc key point 6) |
| host-denied (SpawnsProcess) | 0 | carries `SpawnsProcess`; `class_permits(AgentControl, [SpawnsProcess])` is false and Elevated is a v1 non-goal, so it is not exposable on the v1 facade at any scope (`remote_server/registry.rs` detector-(c) note) |
| v1-deferred (Elevated) | 0 | ledgered Elevated without SpawnsProcess — reachable only under `ui:elevated`, which §1 excludes from v1; deferred, not denied |
| v1-audit-refused (per-command finding) | 0 | the class/capability pair would admit a v1 scope, but a recorded audit found a property of the command AS IT STANDS that no v1 scope can accommodate — fail-open, spawn-capable machinery built to serve a read, an unrenderable transport shape, or a registered remote twin that already answers the query. Never used for arming/steering/write refusals: the facade serves 16 `agentControl` ops, so those stay register-candidates |
| orphan invoke (no local handler) | 0 | invoked by the frontend but absent from `generate_handler!` and from the ledger — it cannot be registered remotely because it does not exist locally either |

**248 invoked names now resolve through the manifest** — host-side commands the facade denies or defers, classified by their ledger row's `v1Resolution` rather than by a registration or a client-local reason (phase-doc key point 6). B0 landed that mechanism and the gap fell 419 → 48 with zero registrations. **What remains in the baseline is registration work only**, so from here every batch's delta is exactly the count it registers.

**48 names are registration candidates**, and `register-candidate` means eligible for a hand audit, not approved: detector (c) has already rejected ledgered-`AgentControl` commands whose process authority the manifest cannot see (`resume_task`, `apply_proposals_to_kanban`, `set_agent_conversation_workspace_auto_publish`). Expect a non-empty rejection subset in every registration batch.

## 3. Recommended batch order

| # | Batch | Title | Cmds | Register-candidates | Not registering | Modules |
|---|---|---|---|---|---|---|
| 1 | `B0` | P-11 third-disposition mechanism (prerequisite, no registrations) | 0 | 0 | 0 | 0 |
| 2 | `B1` | Task core — lifecycle, steps, execution, gates | 19 | 19 | 0 | 3 |
| 3 | `B2` | Chat + agent conversation surface (unblocks PR 3.2) | 19 | 19 | 0 | 3 |
| 4 | `B3` | Review, QA, merge pipeline, validation | 2 | 2 | 0 | 1 |
| 5 | `B4` | Ideation, plans, methodology, workflow | 0 | 0 | 0 | 0 |
| 6 | `B5` | Automation, research, metrics, activity | 0 | 0 | 0 | 0 |
| 7 | `B6` | Personas, role defaults, MCP policy, review settings | 8 | 8 | 0 | 2 |
| 8 | `B7` | Artifacts, task context, notifications, app chrome | 0 | 0 | 0 | 0 |
| 9 | `D1` | Credential + integration surface (disposition only, no registrations) | 0 | 0 | 0 | 0 |
| 10 | `D2` | Process-launch getters and git/gh surface (disposition only) | 0 | 0 | 0 | 0 |
| 11 | `R1` | `get_project` / `list_projects` — spawn-free read path | 0 | 0 | 0 | 0 |
| 12 | `D3` | Host chrome, terminal, repository settings, test data (disposition only) | 0 | 0 | 0 | 0 |
| 13 | `A1` | Chat attachments — disposition + remote rendering (deferred from 2.6/review-4) | 0 | 0 | 0 | 0 |
| 14 | `X1` | Orphan invokes — no local handler exists | 0 | 0 | 0 | 0 |

Ordering logic: **B0 first** (nothing is measurable without the third disposition) → **B1** (smallest parity risk, reuses 1.5-A's proven injection shapes) → **B2** (unblocks PR 3.2, which cannot start until chat send answers `REMOTE_FORBIDDEN` instead of `REMOTE_COMMAND_UNAVAILABLE`) → **B3–B7** registration batches by falling audit risk → **D1/D2/D3** disposition-only batches, which retire large blocks with zero registration risk and can run in parallel with any registration batch once B0 lands → **R1** (a code change, not a registration, and gated on an owner call) → **A1** (blocked on 1.5-C) → **X1** (live defects, independent of remote work).

## 4. Batches

### 1. `B0` — P-11 third-disposition mechanism (prerequisite, no registrations)

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Why here:** LANDED (PR 3.1-b batch B0). The drift scan used to admit two answers — remote-registered, or client-local with a reason. 162 of the then-419 gap names were neither and never will be: host commands the facade denies (Denied class, SpawnsProcess) or defers (Elevated), and writing them into `local-only-commands.ts` would have put a false statement in a file whose whole value is that its reasons are true. `ralphx_remote_protocol::v1_resolution` now derives the verdict from the ledger row, `capability_ledger_tests` renders it as `v1Resolution` on every manifest row, and the scan reads it as a third classification source. The ratchet moved 419 → 257 with zero registrations. Every later batch's delta is now measurable.

**Work:**

- DONE — `v1_resolution(class, capabilities)` in `ralphx-remote-protocol` derives one of `registerable` / `host-denied` / `host-denied-spawns-process` / `v1-deferred`. The ledger row is the authority; nothing downstream re-derives `class_permits`.
- DONE — the `Elevated`/v1-deferred disposition rides the SAME manifest path under a distinct reason code, not a side list and not `local-only-commands.ts` (key point 6). CI shrinks it as Elevated rows are reclassified.
- DONE — 9 new scan self-test cases (26 → 35): each refusal class classifies, a registerable name does not, a name absent from every source stays unclassified, an unknown resolution literal throws, a registered-and-refused row throws, and an absent/shapeless/field-less manifest classifies nothing.
- DONE — the ratchet held: the baseline shrank 419 → 257 and is still delete-on-zero.
- NOTE — `host-only-ux` needed no separate annotation list: all 162 manifest-resolvable names carry a Denied or Elevated ledger row already, so the census's taxonomy covers the set with no side file.

**Gate:** MET — scan self-test 26 → 35 cases; the PASS line reports 190 manifest-classified and the unclassified count fell 419 → 257, exactly the 162-name manifest-resolved set, with zero registrations.

### 2. `B1` — Task core — lifecycle, steps, execution, gates

**Commands:** 19 · **Register-candidates:** 19 · **Risk classes:** register-candidate 19

**Why here:** The 1.5-A surface already registered the neighbouring commands (`move_task`, `unblock_task`, `answer_user_question`, the brakes), so the injection table, the `authz:` predicate shape and the P-4 parity rows for these argument shapes are proven on this exact module family. Lowest parity risk, highest reuse — the right batch to shake out the per-batch harness before it meets 41-command modules.

**Work:**

- Hand-audit each command's downstream authority (detector (a) transitions, detector (b) spawn-triggering state writes, content-surface writes) and assign class + capability set in `capability_ledger.rs`.
- P-4 parity rows FIRST (flat args, struct-wrapped, camelCase, `Option`, error path) per C-11.
- Confirm the brakes in these modules stay `ui:operate` (A-14) and that no arming transition lands below the `AgentControl` floor.
- DONE (PR 3.1-b batch 10): `question_commands` and `permission_commands` are fully classified and no longer appear in this batch's module list. `resolve_user_question` was corrected in place to `Elevated`/`SpawnsProcess` (`host-denied-spawns-process`) after it was measured reaching `resolve_git_cli_path`, `resolve_node_cli_path` and `find_codex_cli_candidates` while sitting at `AgentControl` — an authority-INCREASING correction that preserved its `steering-question` declared membership. `resolve_permission_request` is `seam-resolved-via-remote-twin`: the facade already registers that exact fn twice, as `approve_permission_request` (AgentControl) and `deny_permission_request` (Operate), with the decision field server-pinned, so registering the raw name would move branch selection to a client-supplied argument.

**Gate:** P-17 suite green; P-17b generated scope entries exist for every new AgentControl member; C-9 dual-lens review recorded.

<details><summary>Members by module</summary>

- **`execution_commands`** (6) — `recover_task_execution`, `resolve_recovery_prompt`, `restart_task`, `resume_execution`, `update_execution_settings`, `update_global_execution_settings`
- **`task_commands`** (8) — `archive_task`, `pause_execution_plan`, `restore_task`, `resume_execution_plan`, `resume_task`, `resume_tasks_in_group`, `retry_branch_update`, `stop_execution_plan`
- **`task_step_commands`** (5) — `complete_step`, `fail_step`, `reorder_task_steps`, `skip_step`, `start_step`

</details>

### 3. `B2` — Chat + agent conversation surface (unblocks PR 3.2)

**Commands:** 19 · **Register-candidates:** 19 · **Risk classes:** register-candidate 19

**Why here:** PR 3.2's whole premise is that chat send paths answer `REMOTE_FORBIDDEN` without `ui:agent` rather than `REMOTE_COMMAND_UNAVAILABLE` — which requires them registered. 2.6 shipped the honest interim (composer renders UNAVAILABLE remotely) and its product note says it 'flips with no client change when 3.1 registers them'. This is the batch that flips it, so it must land before 3.2 starts. It is also the highest-risk batch: `send_message` is a detector-(a) steer sink and the module contains the workspace-publish `git push` surface that stays denied.

**Work:**

- Split the module by authority: the send/steer commands register as `AgentControl`; the publish/PR surface (`publish_agent_conversation_workspace`, `update_agent_conversation_workspace_from_base`, `close_agent_workspace_pr`) stays denied, and `set_agent_conversation_workspace_auto_publish` is an already-proven detector-(c) rejection.
- Verify per command that the process-launch sink sits BEYOND the steer-sink cut (`chat_service.send_message`) rather than inside the command's own closure — the cut is what makes chat send registerable while `resume_task` is not. Any command whose own closure resolves a CLI path is a detector-(c) rejection, not a registration.
- P-4 rows must cover `SendAgentMessageInput`'s optional/override fields (the `runtimeOverride` vs legacy-field rejection is an error-path parity row).
- DONE (PR 3.1-b batch 3): `conversation_stats_commands` — all four usage-aggregate reads registered at `ui:read`, so the module no longer appears in this batch's module list. Batch 3's `probe_b2_module_batch_audit` also published detector output for every remaining B2 member; start from it rather than re-deriving. Its headline finding: `get_agent_conversation`, `get_agent_conversation_messages_page` and `get_agent_conversation_timeline_page` — the three transcript reads PR 3.2 needs — all fire detector (a), so they are NOT free reads and need their own hand-trace.
- DONE (PR 3.1-b batch 9): `agent_sidebar_commands` — `list_agent_sidebar_conversations` resolved as `host-denied-spawns-process`, so the module no longer appears in this batch's module list. Batch 9 also closed eight more B2 members by manifest classification rather than registration: `send_agent_message`, `start_agent_conversation`, `get_agent_conversation_workspace`, `list_agent_conversation_workspaces_by_project`, `get_agent_conversation_workspace_freshness`, `is_chat_service_available`, `is_agent_running`, `get_agent_running_states` and `get_agent_conversation_runtime_statuses` all measurably resolve a CLI path in their OWN closure — which is precisely the detector-(c) rejection this batch's work list predicted, now recorded in the ledger instead of only in a pin. `agent_composer_commands` is also fully retired: batch 8 registered `search_agent_composer_plan_references` at `ui:read` and batch 9 resolved `search_agent_composer_entries` (`host-denied-spawns-process`) and `list_agent_composer_skills` (`v1-audit-refused`, fail-open that reports DISABLED skills as enabled).
- READ FIRST — `send_agent_message` and `start_agent_conversation` are ledgered `Elevated`/`SpawnsProcess` as of batch 9, so the split-by-authority plan above no longer applies to them unmodified. PR 3.2's premise (chat send answers `REMOTE_FORBIDDEN` rather than `REMOTE_COMMAND_UNAVAILABLE`) needs the process-launch sink moved BEYOND the command's own closure first — the `list_remote_*`/`get_remote_*` seam split is the proven shape for that. Registering them as they stand would fail `detector_c_floors_process_spawn_authority`.

**Gate:** P-17 green; C-9 dual-lens review recorded; the five 2.6-surfaced ops resolve per this census's `resolvedItems.unregisteredUiAgentOps`.

<details><summary>Members by module</summary>

- **`agent_model_commands`** (1) — `upsert_custom_agent_model`
- **`conversation_folder_reference_commands`** (2) — `add_conversation_folder_reference`, `remove_conversation_folder_reference`
- **`unified_chat_commands`** (16) — `abort_seeded_agent_conversation`, `archive_agent_conversation`, `commit_agent_conversation_workspace_locally`, `create_agent_conversation`, `fork_agent_conversation`, `precompute_agent_conversation_workspace_pr_description`, `reconcile_agent_conversation_workspace_publication`, `restore_agent_conversation`, `send_queued_agent_message_now`, `set_agent_conversation_workspace_auto_publish`, `set_agent_conversation_workspace_pr_supervision`, `stop_agent`, `switch_agent_conversation_mode`, `switch_agent_conversation_persona`, `update_agent_conversation_coordination_mode`, `update_agent_conversation_title`

</details>

### 4. `B3` — Review, QA, merge pipeline, validation

**Commands:** 2 · **Register-candidates:** 2 · **Risk classes:** register-candidate 2

**Why here:** Approval/review commands write the agent-consumed content surface (`MutatesAgentConsumedContent` already appears on 6 of them), which is exactly the capability whose floor P-17d enforces. Grouping them keeps that audit in one review rather than spread across batches.

**Work:**

- Confirm every content-surface writer keeps `MutatesAgentConsumedContent` and lands at or above the AgentControl floor.
- Check the merge-pipeline members against the destructive-git deny list (`cleanup_task_branch`, `resolve_merge_conflict` are Denied and must not ride in on module similarity).
- DONE (PR 3.1-b batch 7): `merge_pipeline_commands` — all three hydration/projection reads registered at `ui:read`; `validation_commands` — `get_task_validation_summary` resolved as `host-denied-spawns-process`. Neither module appears in this batch's module list any more. Batch 7 also registered the `review_commands`/`qa_commands` read cluster (11 rows) and published `probe_b3_module_batch_audit`; start from its detector output rather than re-deriving.
- READ FIRST — batch 7's audit-graph fix changes what a clean probe means. `resolve_dispatch` used to drop every call inside a `commands/` file whose name matched a registered command, which deleted the command→same-named-service delegation edge and made detectors (a)/(b)/(c) vacuously silent for 92 command names. Verdicts taken before that fix are not evidence. `get_task_validation_summary` is the worked example: clean on all three detectors, and shelling out to `git rev-parse HEAD` the whole time.
- OPEN — a second scanner-scope gap is recorded but NOT fixed: `load_production_sources` walks `src-tauri/src` only, so entity methods defined in the `ralphx-domain` crate are invisible and every call to one falls into the resolver's all-same-name fallback. That is what makes `reopen_issue` read as a detector-(c) spawner when its body is a repository read plus an update. It is refused rather than registered, and deliberately NOT ledgered `SpawnsProcess`, so it stays in the gap until the crate scope is widened.
- DONE (PR 3.1-b batch 10): `qa_commands` is fully classified and no longer appears in this batch's module list. `retry_qa` and `update_qa_settings` registered at `ui:agent` — the latter with a declared `arms-auto-qa` membership, because it arms through an in-memory `RwLock` that no detector watches — and `skip_qa` is `v1-audit-refused`: it writes every step as `QAStepResult::skipped`, but `QAResults::from_results` then derives `Pending` rather than `Passed`, contradicting the body's own comment. That discrepancy is a live product bug, not only a facade finding.

**Gate:** P-17d floor diff clean; C-9 review recorded.

<details><summary>Members by module</summary>

- **`review_commands`** (2) — `approve_fix_task`, `reject_fix_task`

</details>

### 5. `B4` — Ideation, plans, methodology, workflow

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B4`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. Disposition-only from the start — the manifest classification IS the disposition.

**Why here:** The largest single module in the gap (42). It is also where the known detector-(c) rejection `apply_proposals_to_kanban` lives, so the batch must be sized to absorb a mid-batch reclassification without stalling the others.

**Work:**

- Expect a non-empty detector-(c) rejection subset; record each rejection in the manifest disposition rather than downgrading the class.
- `delete_task_proposal` is Denied (deletesEntity) — it stays a manifest disposition inside this batch.
- DONE (PR 3.1-b batch 11): the B4 remainder is dispositioned — 19 reads registered at `ui:read`, 14 writers at `ui:agent`, 7 `v1-audit-refused`, 12 `host-denied-spawns-process`. `agent_plan_commands`, `methodology_commands` and `workflow_commands` are fully classified and no longer appear in this batch's module list.
- READ FIRST — batch 11 hand-traced all twelve detector-(c) hits instead of accepting the probe boolean, and correctly established that all twelve reach a real `Command::new`, so the floor excluded none. `activate_agent_task_pipeline` and `activate_agent_plan_direct_implementation` reach it ONLY through the stale-publish repair probe and are recorded as NARROW, which batch 12 re-confirmed by reconstructing the edge chain.
- CORRECTION (PR 3.1-b batch 12) — batch 11 also recorded TWO scanner errors as fact, and NEITHER reproduces. `resolve_manual_role_spawn_settings` and `find_node_cli_path`/`ensure_resolved_node_bin_in_path`/`resolved_node_bin_dir` are all launch-free by the engine's own measurement, so the engine agreed with the hand trace all along. The `codex`/`node` tokens batch 11 called artifacts riding on a git command are REAL, and arrive through `CodexCliClient::spawn_agent -> build_codex_internal_mcp_overrides -> find_node_binary`. Do not inherit the artifact claim. The genuine over-attribution is a third mechanism: callees resolve by BARE NAME, so `conn.execute(..)` binds to `AgentWorkflowRunner::execute`. It is pinned by `batch12_detector_attribution_limits_are_measured_not_assumed` and deliberately not fixed — narrowing resolution removes edges, and edges are what the floor is measured from.
- OPEN — the highest-value fail-open fix in the gap is `ideation_harness_availability.rs:344/:360`: `.ok().flatten()` plus an infallible resolver makes a lane-settings DB error indistinguishable from 'no row configured', so a lane configured to an unavailable Codex reports the Claude default as `available: true`. One propagation fix clears BOTH `get_agent_harness_availability` and `get_ideation_harness_availability`.

**Gate:** P-17 green; C-9 review recorded; rejected members appear as manifest dispositions, never as local-only rows.

### 6. `B5` — Automation, research, metrics, activity

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B5`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. Disposition-only from the start — the manifest classification IS the disposition.

**Why here:** Automation run/restart are two of the five 2.6-surfaced ops; the rest are read-shaped commands that were swept to the conservative module default and are the cheapest reclassification wins in the gap.

**Work:**

- Re-audit the conservative-module-default rows: a genuinely inert read here may drop to `Read`/`Operate`, but only with sink evidence — the floor may not be undershot.
- DONE (PR 3.1-b batch 12): all 33 B5 ratchet members are dispositioned — 18 reads registered at `ui:read`, 12 writers at `ui:agent` (4 of them arming, 1 carrying `SeedsSpawnTriggeringState` and 3 carrying `DECLARED_MEMBERSHIPS` rows), 3 `host-denied-spawns-process`. `activity_commands`, `automation_commands`, `metrics_commands` and `research_commands` are fully classified.
- RESOLVED — the plan asked whether `trigger_automation_run_now` / `restart_automation` have arming targets visible to detector (a). They do not, and the two commands are NOT alike. `trigger_automation_run_now` reaches a real Codex spawn (`dispatch_automation_run_now_action -> spawn_automation_judge_task -> invoke_automation_utility_agent -> CodexCliClient::spawn_agent`) and is refused at the floor with `retry_automation_judge`, which shares that chain. `restart_automation` spawns nothing; it flips `automations.status` to Active, the armed value `spawn_automation_scheduler` scans, and detector (b) misses it because that surface's sole write marker is `reopen_run_corrective`. It is registered at `ui:agent` with a `DECLARED_MEMBERSHIPS` row — NOT with `SeedsSpawnTriggeringState`, which `seeds_spawn_triggering_state_tags_track_detector_b_evidence` defines as detector-(b) evidence — as are `retry_automation_plan_judge` and `skip_automation_judge`. Only `resume_automation_run`, which the detector does flag, earns the capability.
- NOTE for successors — the four automation arming writes were NOT bought by widening the `automation-active` write-marker list. Markers are matched against every command's closure, so a broader marker moves the floor for members batches 7-11 already dispositioned. Declare the membership instead.

**Gate:** P-17 green; C-9 review recorded.

### 7. `B6` — Personas, role defaults, MCP policy, review settings

**Commands:** 8 · **Register-candidates:** 8 · **Risk classes:** register-candidate 8

**Why here:** Configuration-of-future-authority shapes cluster here: a persona/role/policy write does not act now but changes what a later spawn is allowed to do. This is the `update_custom_analysis` family of risk (§3.3 backstop-1 residual), so it gets one focused dual-lens review instead of being sprinkled across batches.

**Work:**

- For each command ask the deferred-authority question explicitly: does this write change what a FUTURE agent process may do? If yes it is at least `AgentControl` with `ConfiguresFutureProcessAuthority`, regardless of how inert the immediate action looks.
- `delete_persona`-shaped members stay Denied (deletesEntity).
- DONE (PR 3.1-b batch 13): `persona_commands` (12) and `mcp_policy_commands` (7) are fully classified — 8 reads at `ui:read`, 12 writers at `ui:agent` (8 carrying `MutatesAgentConsumedContent`, 4 carrying `DECLARED_MEMBERSHIPS`), 3 `host-denied-spawns-process`.
- RESOLVED, and successors must not re-litigate it — the deferred-authority lens above says such a write is 'at least AgentControl with ConfiguresFutureProcessAuthority'. That reading is UNREPRESENTABLE: `class_permits` admits `ConfiguresFutureProcessAuthority` only under `Elevated`, which v1 grants no scope for, so declaring it converts an audited-clean bounded write into a deferral by notation rather than by finding. The idiom that records the same finding at a registerable class is AgentControl plus a `DECLARED_MEMBERSHIPS` row, which is what `update_agent_lane_settings` already carries for picking the harness a live agent is launched with — strictly more deferred authority than an MCP server/tool override. Batch 13 used declarations `configures-future-agent-tool-authority` and `configures-future-agent-capability-gates`.
- READ FIRST — `get_mcp_catalog` and `refresh_mcp_catalog` are REFUSED at the floor. They are reads by intent, but `build_catalog -> discover_provider_catalog -> resolve_codex_catalog_cli_path` launches the Codex app-server to answer. `retry_legacy_mcp_registration_repair` is ALSO refused, and detector (c) does NOT see it: it runs `claude mcp remove ralphx -s user` through `tokio::process::Command::new`, hidden by a `spawn_blocking(bare_fn)` call shape that creates no edge plus a spawn on an already-resolved path that names no resolver. Pinned by `batch13_detector_gap_is_measured_not_inherited`.

**Gate:** P-17 green; C-9 review recorded with the deferred-authority lens explicitly exercised.

<details><summary>Members by module</summary>

- **`manual_role_default_commands`** (6) — `clear_manual_role_default`, `get_agent_conversation_role_default`, `get_manual_role_defaults`, `get_start_composer_role_default`, `reset_agent_conversation_role_default`, `update_manual_role_default`
- **`workspace_review_settings_commands`** (2) — `get_workspace_review_runtime_settings`, `update_workspace_review_runtime_settings`

</details>

### 8. `B7` — Artifacts, task context, notifications, app chrome

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B7`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. Disposition-only from the start — the manifest classification IS the disposition.

**Why here:** The tail. Mixed reads and small writes; also the batch that must decide which names are genuinely CLIENT-LOCAL (updater channel, window/dock chrome) and therefore belong in `local-only-commands.ts` with an honest reason — the only batch expected to add local-only rows.

**Work:**

- Split client-local from host-owned per command: `update_channel_commands` and parts of `ui_commands` are plausible `local-only` rows; artifacts and task context are host state and must register or be manifest-disposed.
- `get_task_context` and the prompt-builder reads are content-surface members (ledger-soundness round found 5 dropped worker content reads) — re-check the surface enumeration before assigning.
- Every local-only row gets an honest client-local reason; 'hard to classify' is never valid.
- DONE (PR 3.1-b batch 13): all 33 B7 ratchet members are dispositioned — 24 reads at `ui:read`, 8 writers at `ui:agent`, 1 `v1-deferred`. Zero local-only rows were added, which answers the batch's own open question: the client-local split it anticipated did not survive contact with the commands.
- RESOLVED — the batch was expected to move `update_channel_commands` and parts of `ui_commands` to `local-only-commands.ts`. Neither is client-local. `get_update_channel`/`set_update_channel` read and write `app_state_repo`, which is HOST state, and `get_ui_feature_flags` projects the host runtime config plus the agent-capability snapshot. `set_update_channel` is instead ledgered `Elevated`/`HostManagement` (V1Deferred, not denied): it selects which release train the desktop app auto-updates onto, matching every other HOST row's class. Its read half is registered.
- RESOLVED — the plan flagged 5 dropped worker content reads. They are the `task_context_commands` Tauri commands (`get_task_context`, `get_artifact_full`, `get_artifact_version`, `get_related_artifacts`, `search_artifacts`), all registered at `ui:read`. Note their HTTP namesakes in `http_server/handlers/worker.rs` are DIFFERENT functions: the axum `search_artifacts` silently skips unparsable artifact types, while the Tauri command propagates the parse error. Do not reason about one from the other.
- NOTE — batch 12 measured this block detector-silent and batch 13 re-measured rather than inheriting, which is how it found that detector (b)'s flag on `update_notification_settings` is a bare-name MARKER collision (`update_settings` vs the workspace-auto-review write marker), not a spawn-triggering write. It is registered WITHOUT `SeedsSpawnTriggeringState`; claiming the tag would have passed the evidence test while being false.

**Gate:** P-17 green; every new local-only row has a reason; C-9 review recorded.

### 9. `D1` — Credential + integration surface (disposition only, no registrations)

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B0`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. Disposition-only from the start — the manifest classification IS the disposition.

**Why here:** Every member is `TouchesCredentials` or `ConfiguresFutureProcessAuthority`. API-key management is compile-denied from the facade (§4.3) and the integration-settings saves are the round-3 module deny list. Nothing here registers in v1; the entire batch is manifest disposition, so it is pure throughput once B0 lands — 72 names retired with zero registration risk.

**Work:**

- Confirm each ledger row already carries the denying capability; add missing rows rather than adding local-only reasons.
- The ticketing reads are Elevated-not-Denied (they read a credentialed provider): decide once, for the whole module, whether v1 defers them or the reads split from the writes in a later phase. Record the decision in the ledger reason.

**Gate:** Manifest regenerated and diff-clean; unclassified count drops by exactly this batch's size; zero new local-only rows.

### 10. `D2` — Process-launch getters and git/gh surface (disposition only)

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B0`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. Disposition-only from the start — the manifest classification IS the disposition.

**Why here:** The 'getter that shells out' family plus the destructive-git and installer surfaces. `SpawnsProcess` is not exposable at any v1 scope, so these are dispositions, not registrations. `get_project`/`list_projects` are carved out into R1 because they are the one case where the spawn is removable rather than inherent.

**Work:**

- Verify each row carries `SpawnsProcess` (detector (c) is the floor: a Read/Operate row reaching a launch sink fails CI).
- `get_task_file_changes` / `get_file_diff` / `get_codex_cli_diagnostics` are the named getter-spawns — they stay denied even though they read like reads.

**Gate:** Manifest diff-clean; detector-(c) floor test green; unclassified count drops by exactly this batch's size.

### 11. `R1` — `get_project` / `list_projects` — spawn-free read path

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B0`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. NOT closed, though: leaving the ratchet is a bookkeeping fact, not an answer. Both names are manifest-classified `host-denied-spawns-process` because the getter shells out TODAY; §5.1's open question is whether to remove the spawn so they can be registered, and that owner call still stands. If it is answered yes, these rows change class and re-enter as registration work.

**Why here:** The only commands in the gap whose process authority is INCIDENTAL. Both are pure repository reads; the single spawning field is `repository_capability`, computed per project by shelling out to git in `project_response()`. Removing that inline shell-out makes the highest-traffic read on the whole remote surface registerable as `Read`. See `resolvedItems.projectGetters` for the proposed path and the rejected alternatives.

**Work:**

- Land the cache-backed capability read (option A in `resolvedItems.projectGetters`) as its own change with its own tests — NOT inside a registration batch.
- Only after the shell-out is gone: re-run detector (c), drop the `SpawnsProcess` capability, reclassify to `Read`, register both.
- If option A is rejected by the owner, both names fall back into D2 as v1-deferred dispositions and the frontend project list stays a fetch-route question (3.1 open question 4).

**Gate:** Detector (c) reports no launch sink in either closure; P-4 parity rows for both; the manifest shows class `read` with an empty capability set.

### 12. `D3` — Host chrome, terminal, repository settings, test data (disposition only)

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B0`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. Disposition-only from the start — the manifest classification IS the disposition.

**Why here:** Terminal is the phase doc's worked example of the third disposition: its invoke names resolve for P-11 through the module-`Denied` (`PtyControl`) rows, NEVER through a client-local reason. Test data is hard-denied outright (total-data-loss blast radius). Startup/repository settings are `HostManagement`/`ConfiguresFutureProcessAuthority` — v1-deferred.

**Work:**

- Assert the terminal names resolve through the manifest path introduced in B0; a `local-only` row for any of them is a defect, not a shortcut.
- Keep `report_startup_frontend_milestone` honest: it is a client-originated report about the LOCAL app boot — check whether it is genuinely client-local (local-only row) rather than host-deferred.

**Gate:** Manifest diff-clean; a planted local-only row for a terminal command fails CI.

### 13. `A1` — Chat attachments — disposition + remote rendering (deferred from 2.6/review-4)

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Retired by `B0`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. NOT closed: the attachment names leave the ratchet, but remote attachment RENDERING is a fetch route, not an invoke command, and §5.3's `ChatAttachmentGallery.tsx` gap plus the 1.5-C endpoint dependency are untouched by B0.

**Why here:** 2.6-a shipped the honest interim: under a remote environment `getImagePreviewSrc()` returns `null` and every attachment renders as a placeholder card, because `convertFileSrc` mints an `asset://` URL for a path on the CLIENT's filesystem while `attachment.filePath` names a file on the HOST. The comment at `MessageAttachments.tsx:92-94` defers the real fix to 3.1. The three attachment commands are Denied (`writesArbitraryPath` / `deletesEntity`) and stay dispositions — the rendering work is a FETCH-path change, not a command registration, which is exactly 3.1 open question 4 and needs an explicit call.

**Work:**

- Blocked on 1.5-C: `/remote/v1/attachments/{id}` does not exist on this base (no `attachments` route in `remote_server/`). Do not start A1 until the 1.5 lane lands it.
- Branch preview-source resolution on env kind in BOTH renderers — `MessageAttachments.tsx:115` and `ChatAttachmentGallery.tsx:97` (2.6 only hardened the first; the gallery still calls `convertFileSrc` unconditionally, which is a live gap this census surfaces).
- Route the remote branch through the scoped endpoint under `ui:read` with a binary-safe body and 2.7's response-header envelope; never through JSON `/invoke` (C-16).
- Resolve open question 4 explicitly: extending the §3.5 fetch-route remount allowlist rides 3.1, or it is a separate change against P-1's checked-in allowlist. Record the call.
- Disposition the three commands through the manifest (upload/delete are `writesArbitraryPath` fs sinks; `list_message_attachments` is denied by module).

**Gate:** A remote attachment renders through the scoped endpoint; a local one still uses `convertFileSrc`; P-1 route-allowlist equality still holds; the three commands are manifest-disposed with zero local-only rows.

### 14. `X1` — Orphan invokes — no local handler exists

**Commands:** 0 · **Register-candidates:** 0 · **Risk classes:** —

**Why here:** RESOLVED in PR 3.1-b. Five gap names were absent from `src-tauri/src/commands/registry.rs` `generate_handler!` AND from the ledger (which is exhaustive over it), so every call rejected at runtime with no remote environment involved. The reachability audit found none of the five was wired to any component or event handler — the wrappers were dead code kept alive only by their own unit tests — so all five were resolved by DELETING the call site rather than by minting host authority the product does not use. This batch is now empty and stays here as the record of that call.

**Work:**

- `add_proposal_dependency` — deleted at both call sites (`api/ideation.ts` `dependencies.add`, `api/proposal.ts` `addProposalDependency`) plus the `addDependency` mutation in `hooks/useDependencyGraph.ts`. Its owning hook `useDependencyMutations` had no consumer at all: the UI reads the dependency graph and never writes edges, so there was no product asymmetry to fix by adding the missing command.
- `create_child_session` / `get_parent_session_context` — deleted from `api/ideation.ts` (zero callers, zero tests). The capability is not lost: both are live HTTP routes (`POST /api/create_child_session`, `GET /api/parent_session_context/:session_id`), which is how the backend actually reaches them.
- `delete_project` — deleted from `api/projects.ts`; `projectsApi.archive` is the live removal path.
- `delete_task` — deleted from `api/tasks.ts` and `hooks/useTaskMutation.ts`; it was already `@deprecated Use cleanupTask instead`, and every component destructures `cleanupTaskMutation`.
- Regression guard: `frontend/src/api/orphan-invokes.test.ts` asserts each wrapper stays absent while its surviving sibling (`remove`, `getChildren`, `archive`, `cleanupTask`) stays present, so the test cannot pass by the namespace disappearing.

**Gate:** Each of the five is deleted at the call site with a regression test; the P-11 scan sees zero orphans.

## 5. Resolved items

### 5.1 `get_project` / `list_projects` — the getter that shells out

**Status:** proposal — needs an owner call before 3.1-b starts R1 · **Batch:** `R1`

**Finding.** Both are pure repository reads (`project_commands.rs:211-240`): `project_repo.get_all()` / `get_by_id()`, then `project_response()` per row. The ONLY process authority is one response field — `repository_capability`, produced by `inspect_repository_capability()` (`infrastructure/git_auth.rs`), which runs `git remote get-url origin` and `git remote get-url --push origin` through `resolve_git_cli_path()` with a 5s deadline, once PER PROJECT. That is what makes a getter a `SpawnsProcess`/Elevated command, and it is incidental to the read, not inherent to it.

**Proposal.** Option A — cache the capability, do not compute it in the getter. Persist the inspected `repository_capability` (plus `inspected_at`) alongside the project row, write it from the paths that already have process authority and already shell out (project create/update, `change_project_git_mode`, `setup_gh_git_auth`, `switch_git_origin_to_ssh`, `reanalyze_project`) plus one background refresh whose loop root is declared in the manifest's `background_loop_inventory`, and have `project_response()` READ the cached value. `list_projects`/`get_project` then hold no launch sink in their closure, detector (c) goes quiet, the `SpawnsProcess` capability drops, and both classify as `Read` — registerable on the v1 facade at `ui:read`, with zero `generate_handler!` edits and zero command-fn forks (A-7). The response shape is unchanged, so P-4 parity and every existing caller are untouched; only the freshness semantics change, and a stale-capability value is strictly safer than the current InspectionFailed-on-timeout behaviour (`inspect_repository_capability` already returns `InspectionFailed{message}` rather than erroring, so consumers already handle a non-authoritative value).

**Rejected alternatives:**

- A response projection that omits `repository_capability` for remote callers — that is a command-fn fork, which A-7 forbids, and it would break P-4 byte-identity between local IPC and remote dispatch (the whole point of the parity suite).
- A pinned facade op — pins fix ARGUMENTS (`approve_permission_request` / `deny_permission_request`), not response shape; there is no pin that removes a field.
- Registering as Elevated — `ui:elevated` is a §1 v1 non-goal; this would ship a scope nothing can hold.
- Serving the project list over a remounted fetch route instead — `http_server/handlers/projects.rs` computes the SAME capability inline, so the spawn moves rather than disappears, and it opens 3.1 open question 4 unnecessarily.

**If the owner rejects option A.** Both names fall back to D2 as v1-deferred dispositions. That is not cost-free: the project list is the entry point of nearly every remote screen, so a remote client would have to hydrate projects through a fetch route (open question 4) or run with no project list at all.

### 5.2 The five 2.6-surfaced unregistered `ui:agent` ops

**Status:** PARTIALLY RESOLVED (PR 3.1-b batch 9) — the detector-(c) confirmation this section made mandatory was run. `skip_step`, `trigger_automation_run_now` and `restart_automation` remain registration candidates; `send_agent_message` and `start_agent_conversation` came back POSITIVE and are now manifest-classified `host-denied-spawns-process`. The evidence bullet below claiming the provider launch sits outside chat send's own closure is therefore WRONG and is retained only as the record of what the static read predicted.

| Command | Ledger class | Capabilities | Batch | Resolution |
|---|---|---|---|---|
| `send_agent_message` | elevated | spawnsProcess | `B2` | DEMOTED (batch 9) — `host-denied-spawns-process`. Detector (c) fires on its OWN closure, which is already cut at the `send_message` steer sink: it still reaches `resolve_git_cli_path`, `resolve_node_cli_path` and `find_codex_cli_candidates` by another route. Registering it would fail `detector_c_floors_process_spawn_authority`. |
| `start_agent_conversation` | elevated | spawnsProcess | `B2` | DEMOTED (batch 9) — `host-denied-spawns-process`. Same three resolvers reached from its own cut closure. |
| `skip_step` | agentControl | agentControl, mutatesAgentConsumedContent | `B1` | register (`ui:agent`), pending detector-(c) confirmation |
| `trigger_automation_run_now` | elevated | spawnsProcess | `B5` | register (`ui:agent`), pending detector-(c) confirmation |
| `restart_automation` | agentControl | agentControl | `B5` | register (`ui:agent`), pending detector-(c) confirmation |

**Briefing correction.** The 3.1-a brief states three of these five are detector-(c)-rejected. That does not match the code: the detector-(c) trio is `resume_task`, `apply_proposals_to_kanban`, `set_agent_conversation_workspace_auto_publish` (`remote_server/registry.rs` NOT-registered note; `frontend/src/lib/remote/agent-gate.test.ts:114-124` uses exactly those three as the unavailable-by-ABSENCE fixture). None of the five 2.6-surfaced ops appears in that set. The two lists were conflated — they are different trios, and 2.6's tracker note lists the five as ops that 'flip with no client change when 3.1 registers them', i.e. registration is the intended resolution.

**Evidence:**

- 2.6 tracker product note: 'with `ui:agent` granted, chat send / start composer / skip_step / automation run+restart render UNAVAILABLE remotely — send_agent_message etc. are unregistered in 1.5-A's 27-op surface. Honest against this build; flips with no client change when 3.1 registers them.'
- Phase 3 doc, PR 3.2 key point 4: 'Chat send paths (`start_agent_conversation`, `send_agent_message` + variants, …) are `AgentControl` — a device without `ui:agent` gets `REMOTE_FORBIDDEN`'. `REMOTE_FORBIDDEN` (not `REMOTE_COMMAND_UNAVAILABLE`) is only reachable for a REGISTERED command, so 3.2 requires these registered.
- All five are ledgered `class: agentControl`, `capabilities: [agentControl]`, reason `conservative-module-default` — none carries `SpawnsProcess`.
- `send_agent_message` reaches `chat_service.send_message` (`unified_chat_commands/mod.rs`), which is a detector-(a) STEER sink. `all_cut_sinks()` CUTS the closure at steer sinks, so the provider process launch beyond it is outside the command's own closure — which is precisely why chat send is registerable while `resume_task` (whose closure resolves a CLI path directly) is not.

**Obligation on 3.1-b.** This is a static read of the call graph, not a detector run. 3.1-b must confirm each of the five against the live P-17 detector-(c) output as the first step of its batch, and demote any that come back positive to a manifest disposition — the class is decided by the detector, never by this census.

**Client impact.** No client change is needed: `agent-gate.ts` derives availability from ABSENCE in `facade_ops`, so each op flips from `unavailable` to `gated`/`enabled` the moment its registration lands in the regenerated manifest.

### 5.3 Remote attachment rendering

**Status:** scoped into batch A1; BLOCKED on the 1.5-C endpoint · **Batch:** `A1`

**Finding.** Deferred here from 2.6-a and the review-4 round. Current behaviour is the honest interim, not a bug: `getImagePreviewSrc()` (`frontend/src/components/Chat/MessageAttachments.tsx:99-116`) returns `null` whenever the active environment is remote, so every host attachment renders as a placeholder card instead of a broken image — `convertFileSrc` would mint an `asset://` URL for a path on the CLIENT's disk while `attachment.filePath` names a file on the HOST.

**Blockers:**

- `/remote/v1/attachments/{id}` does not exist on this base — there is no attachments route in `src-tauri/src/remote_server/`. It is 1.5-C's deliverable (live in the `rme-pr-1-5` lane). A1 cannot start until it lands.
- 2.7's response-header envelope and a binary-safe body are prerequisites; binary must never travel through JSON `/invoke` (C-16).

**New gap this census found.** 2.6 hardened only ONE of the two renderers. `ChatAttachmentGallery.tsx:97` still calls `convertFileSrc(attachment.filePath)` with no env-kind branch, so the gallery surface renders broken images under a remote environment where `MessageAttachments` renders placeholders. A1 must fix both, and the 2.6 negative test (`host-affordance-gating.test.tsx`, which asserts `convertFileSrc` was NOT called) should be extended to cover the gallery.

**Open question.** Phase-3 open question 4 applies verbatim: attachment rendering is a FETCH route, not an invoke command, and the source does not say whether extending the §3.5 remount allowlist rides 3.1 or requires a separate change against P-1's checked-in allowlist. A1 must record the call before it writes a route.

**Command side.** The three attachment COMMANDS in the gap (`upload_chat_attachment`, `delete_chat_attachment`, `list_message_attachments`) are all ledgered `denied` (`writesArbitraryPath` / `deletesEntity`) and stay manifest dispositions — no registration, and specifically no local-only rows.

## 6. Reconciliation

| Check | Result |
|---|---|
| Drift scan passes | yes (this file is not emitted otherwise) |
| Scan unclassified count == baseline size | 48 == 48 |
| Every gap command in exactly one batch | 48 / 48 |
| Disposition totals sum to the gap | 48 == 48 |
| Batch plan claims no empty module and pins no absent command | enforced by the generator |

Machine-readable companion for 3.1-b/c: [`remote-coverage-census.json`](./remote-coverage-census.json) — same batches, plus per-command `{batch, module, ledgerClass, capabilities, disposition}` rows.
