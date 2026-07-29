#!/usr/bin/env node

/**
 * PR 3.1 — P-11 facade-coverage gap census (planning artifact, PR 3.1-a).
 *
 * The phase doc's first 3.1 task is "re-run the P-11 AST scan; emit the coverage gap
 * list (invoked ∧ not registered ∧ not local-only), grouped by backend command module;
 * check the gap list into the PR as the work manifest". This generator IS that emitter.
 * It registers nothing and classifies nothing on its own authority: every fact below is
 * read out of an existing checked-in source, and the batch grouping is the only added
 * judgement (it is curated in `BATCHES`, and validated to partition the gap exactly).
 *
 * Inputs (all read-only):
 *   - `scripts/check-remote-transport-drift.mjs` — run as a subprocess; its PASS line is
 *     the authority for the live totals. If the scan fails or its counts disagree with
 *     the baseline this generator refuses to emit, so the census can never describe a
 *     surface the scan does not see.
 *   - `scripts/remote-transport-drift-baseline.json` — the exact unclassified set the
 *     scan just re-proved (invoked ∧ not remote-registered ∧ not local-only).
 *   - `docs/generated/remote-commands.json` — the capability ledger: owning backend
 *     module, risk class, capability set, and registration state per command.
 *
 * Outputs:
 *   - `docs/generated/remote-coverage-census.md`   — human work manifest.
 *   - `docs/generated/remote-coverage-census.json` — machine companion for 3.1-b/c.
 *
 * Usage:
 *   node scripts/generate-remote-coverage-census.mjs           # write both artifacts
 *   node scripts/generate-remote-coverage-census.mjs --check    # staleness gate (no write)
 */

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
const positional = args.find((arg) => !arg.startsWith("--"));
const repoRoot = path.resolve(
  positional ?? path.join(path.dirname(fileURLToPath(import.meta.url)), "..")
);

const scanPath = path.join(repoRoot, "scripts", "check-remote-transport-drift.mjs");
const baselinePath = path.join(repoRoot, "scripts", "remote-transport-drift-baseline.json");
const manifestPath = path.join(repoRoot, "docs", "generated", "remote-commands.json");
const localOnlyPath = path.join(
  repoRoot,
  "frontend",
  "src",
  "lib",
  "remote",
  "local-only-commands.ts"
);
const mdOutPath = path.join(repoRoot, "docs", "generated", "remote-coverage-census.md");
const jsonOutPath = path.join(repoRoot, "docs", "generated", "remote-coverage-census.json");

function fail(message) {
  console.error(`FAIL: remote coverage census — ${message}`);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Disposition model
// ---------------------------------------------------------------------------

/**
 * What can happen to an unclassified command in PR 3.1, derived MECHANICALLY from the
 * ledger. This is the coarse routing only — the per-command hand audit (§3.3) and the
 * P-17 detector run still own the final class, and `registerCandidate` explicitly does
 * NOT mean "will register": detector (c) has already rejected ledgered-`AgentControl`
 * commands (`resume_task`, `apply_proposals_to_kanban`,
 * `set_agent_conversation_workspace_auto_publish`) whose process-launch authority is
 * invisible in the manifest.
 */
const DISPOSITIONS = {
  registerCandidate: {
    label: "register-candidate",
    rule: "ledgered AgentControl (or lower) with no SpawnsProcess capability — eligible for a hand-audited `remote_commands!` entry under `ui:agent`",
  },
  hostDeniedClass: {
    label: "host-denied (class: denied)",
    rule: "`class_permits` returns false for Denied at any capability set — registering it fails compilation. Resolves for P-11 through the manifest, never through a local-only reason (phase doc key point 6)",
  },
  hostDeniedSpawn: {
    label: "host-denied (SpawnsProcess)",
    rule: "carries `SpawnsProcess`; `class_permits(AgentControl, [SpawnsProcess])` is false and Elevated is a v1 non-goal, so it is not exposable on the v1 facade at any scope (`remote_server/registry.rs` detector-(c) note)",
  },
  v1DeferredElevated: {
    label: "v1-deferred (Elevated)",
    rule: "ledgered Elevated without SpawnsProcess — reachable only under `ui:elevated`, which §1 excludes from v1; deferred, not denied",
  },
  orphan: {
    label: "orphan invoke (no local handler)",
    rule: "invoked by the frontend but absent from `generate_handler!` and from the ledger — it cannot be registered remotely because it does not exist locally either",
  },
};

/**
 * B0 landed, so the disposition is READ off the row's rendered `v1Resolution` rather than
 * re-derived from class + capabilities here. One authority
 * (`ralphx_remote_protocol::v1_resolution`), three consumers.
 */
const RESOLUTION_DISPOSITIONS = {
  "host-denied": "hostDeniedClass",
  "host-denied-spawns-process": "hostDeniedSpawn",
  "v1-deferred": "v1DeferredElevated",
  registerable: "registerCandidate",
};

function dispositionOf(command, ledgerEntry) {
  if (!ledgerEntry) return "orphan";
  const disposition = RESOLUTION_DISPOSITIONS[ledgerEntry.v1Resolution];
  if (!disposition) {
    fail(
      `ledger row \`${command}\` renders v1Resolution \`${ledgerEntry.v1Resolution}\`, which this census cannot route`
    );
  }
  return disposition;
}

// ---------------------------------------------------------------------------
// Batch plan (the curated part)
// ---------------------------------------------------------------------------

/**
 * Module-grouped work batches in recommended execution order. `modules` partitions the
 * ledger modules present in the gap; `commands` pins individual names that are carved
 * out of their module's batch because they carry their own decision.
 */
const BATCHES = [
  {
    id: "B0",
    title: "P-11 third-disposition mechanism (prerequisite, no registrations)",
    modules: [],
    commands: [],
why: "LANDED (PR 3.1-b batch B0). The drift scan used to admit two answers — remote-registered, or client-local with a reason. 162 of the then-419 gap names were neither and never will be: host commands the facade denies (Denied class, SpawnsProcess) or defers (Elevated), and writing them into `local-only-commands.ts` would have put a false statement in a file whose whole value is that its reasons are true. `ralphx_remote_protocol::v1_resolution` now derives the verdict from the ledger row, `capability_ledger_tests` renders it as `v1Resolution` on every manifest row, and the scan reads it as a third classification source. The ratchet moved 419 → 257 with zero registrations. Every later batch's delta is now measurable.",
    work: [
      "DONE — `v1_resolution(class, capabilities)` in `ralphx-remote-protocol` derives one of `registerable` / `host-denied` / `host-denied-spawns-process` / `v1-deferred`. The ledger row is the authority; nothing downstream re-derives `class_permits`.",
      "DONE — the `Elevated`/v1-deferred disposition rides the SAME manifest path under a distinct reason code, not a side list and not `local-only-commands.ts` (key point 6). CI shrinks it as Elevated rows are reclassified.",
      "DONE — 9 new scan self-test cases (26 → 35): each refusal class classifies, a registerable name does not, a name absent from every source stays unclassified, an unknown resolution literal throws, a registered-and-refused row throws, and an absent/shapeless/field-less manifest classifies nothing.",
      "DONE — the ratchet held: the baseline shrank 419 → 257 and is still delete-on-zero.",
      "NOTE — `host-only-ux` needed no separate annotation list: all 162 manifest-resolvable names carry a Denied or Elevated ledger row already, so the census's taxonomy covers the set with no side file.",
    ],
    gate: "MET — scan self-test 26 → 35 cases; the PASS line reports 190 manifest-classified and the unclassified count fell 419 → 257, exactly the 162-name manifest-resolved set, with zero registrations.",
  },
  {
    id: "B1",
    title: "Task core — lifecycle, steps, execution, gates",
    modules: [
      "task_commands",
      "task_step_commands",
      "execution_commands",
      "question_commands",
      "permission_commands",
    ],
    why: "The 1.5-A surface already registered the neighbouring commands (`move_task`, `unblock_task`, `answer_user_question`, the brakes), so the injection table, the `authz:` predicate shape and the P-4 parity rows for these argument shapes are proven on this exact module family. Lowest parity risk, highest reuse — the right batch to shake out the per-batch harness before it meets 41-command modules.",
    work: [
      "Hand-audit each command's downstream authority (detector (a) transitions, detector (b) spawn-triggering state writes, content-surface writes) and assign class + capability set in `capability_ledger.rs`.",
      "P-4 parity rows FIRST (flat args, struct-wrapped, camelCase, `Option`, error path) per C-11.",
      "Confirm the brakes in these modules stay `ui:operate` (A-14) and that no arming transition lands below the `AgentControl` floor.",
    ],
    gate: "P-17 suite green; P-17b generated scope entries exist for every new AgentControl member; C-9 dual-lens review recorded.",
  },
  {
    id: "B2",
    title: "Chat + agent conversation surface (unblocks PR 3.2)",
    modules: [
      "unified_chat_commands",
      "agent_sidebar_commands",
      "agent_composer_commands",
      // `conversation_stats_commands` was a B2 module and is now fully classified — PR 3.1-b
      // batch 3 registered all four usage-aggregate reads at `ui:read`. It is dropped from
      // the plan because the plan enumerates REMAINING gap work; the completion is recorded
      // in `work` below so it does not read as a silently abandoned module.
      "conversation_folder_reference_commands",
      "agent_model_commands",
    ],
    why: "PR 3.2's whole premise is that chat send paths answer `REMOTE_FORBIDDEN` without `ui:agent` rather than `REMOTE_COMMAND_UNAVAILABLE` — which requires them registered. 2.6 shipped the honest interim (composer renders UNAVAILABLE remotely) and its product note says it 'flips with no client change when 3.1 registers them'. This is the batch that flips it, so it must land before 3.2 starts. It is also the highest-risk batch: `send_message` is a detector-(a) steer sink and the module contains the workspace-publish `git push` surface that stays denied.",
    work: [
      "Split the module by authority: the send/steer commands register as `AgentControl`; the publish/PR surface (`publish_agent_conversation_workspace`, `update_agent_conversation_workspace_from_base`, `close_agent_workspace_pr`) stays denied, and `set_agent_conversation_workspace_auto_publish` is an already-proven detector-(c) rejection.",
      "Verify per command that the process-launch sink sits BEYOND the steer-sink cut (`chat_service.send_message`) rather than inside the command's own closure — the cut is what makes chat send registerable while `resume_task` is not. Any command whose own closure resolves a CLI path is a detector-(c) rejection, not a registration.",
      "P-4 rows must cover `SendAgentMessageInput`'s optional/override fields (the `runtimeOverride` vs legacy-field rejection is an error-path parity row).",
      "DONE (PR 3.1-b batch 3): `conversation_stats_commands` — all four usage-aggregate reads registered at `ui:read`, so the module no longer appears in this batch's module list. Batch 3's `probe_b2_module_batch_audit` also published detector output for every remaining B2 member; start from it rather than re-deriving. Its headline finding: `get_agent_conversation`, `get_agent_conversation_messages_page` and `get_agent_conversation_timeline_page` — the three transcript reads PR 3.2 needs — all fire detector (a), so they are NOT free reads and need their own hand-trace.",
    ],
    gate: "P-17 green; C-9 dual-lens review recorded; the five 2.6-surfaced ops resolve per this census's `resolvedItems.unregisteredUiAgentOps`.",
  },
  {
    id: "B3",
    title: "Review, QA, merge pipeline, validation",
    modules: ["review_commands", "qa_commands"],
    why: "Approval/review commands write the agent-consumed content surface (`MutatesAgentConsumedContent` already appears on 6 of them), which is exactly the capability whose floor P-17d enforces. Grouping them keeps that audit in one review rather than spread across batches.",
    work: [
      "Confirm every content-surface writer keeps `MutatesAgentConsumedContent` and lands at or above the AgentControl floor.",
      "Check the merge-pipeline members against the destructive-git deny list (`cleanup_task_branch`, `resolve_merge_conflict` are Denied and must not ride in on module similarity).",
      "DONE (PR 3.1-b batch 7): `merge_pipeline_commands` — all three hydration/projection reads registered at `ui:read`; `validation_commands` — `get_task_validation_summary` resolved as `host-denied-spawns-process`. Neither module appears in this batch's module list any more. Batch 7 also registered the `review_commands`/`qa_commands` read cluster (11 rows) and published `probe_b3_module_batch_audit`; start from its detector output rather than re-deriving.",
      "READ FIRST — batch 7's audit-graph fix changes what a clean probe means. `resolve_dispatch` used to drop every call inside a `commands/` file whose name matched a registered command, which deleted the command→same-named-service delegation edge and made detectors (a)/(b)/(c) vacuously silent for 92 command names. Verdicts taken before that fix are not evidence. `get_task_validation_summary` is the worked example: clean on all three detectors, and shelling out to `git rev-parse HEAD` the whole time.",
      "OPEN — a second scanner-scope gap is recorded but NOT fixed: `load_production_sources` walks `src-tauri/src` only, so entity methods defined in the `ralphx-domain` crate are invisible and every call to one falls into the resolver's all-same-name fallback. That is what makes `reopen_issue` read as a detector-(c) spawner when its body is a repository read plus an update. It is refused rather than registered, and deliberately NOT ledgered `SpawnsProcess`, so it stays in the gap until the crate scope is widened.",
    ],
    gate: "P-17d floor diff clean; C-9 review recorded.",
  },
  {
    id: "B4",
    title: "Ideation, plans, methodology, workflow",
    modules: [
      "ideation_commands",
      "plan_commands",
      "agent_plan_commands",
      "methodology_commands",
      "workflow_commands",
    ],
    why: "The largest single module in the gap (42). It is also where the known detector-(c) rejection `apply_proposals_to_kanban` lives, so the batch must be sized to absorb a mid-batch reclassification without stalling the others.",
    work: [
      "Expect a non-empty detector-(c) rejection subset; record each rejection in the manifest disposition rather than downgrading the class.",
      "`delete_task_proposal` is Denied (deletesEntity) — it stays a manifest disposition inside this batch.",
    ],
    gate: "P-17 green; C-9 review recorded; rejected members appear as manifest dispositions, never as local-only rows.",
  },
  {
    id: "B5",
    title: "Automation, research, metrics, activity",
    modules: ["automation_commands", "research_commands", "metrics_commands", "activity_commands"],
    why: "Automation run/restart are two of the five 2.6-surfaced ops; the rest are read-shaped commands that were swept to the conservative module default and are the cheapest reclassification wins in the gap.",
    work: [
      "Re-audit the conservative-module-default rows: a genuinely inert read here may drop to `Read`/`Operate`, but only with sink evidence — the floor may not be undershot.",
      "`trigger_automation_run_now` / `restart_automation` route through the scheduler seam; confirm the arming-transition targets are visible to detector (a) before assigning.",
    ],
    gate: "P-17 green; C-9 review recorded.",
  },
  {
    id: "B6",
    title: "Personas, role defaults, MCP policy, review settings",
    modules: [
      "persona_commands",
      "manual_role_default_commands",
      "mcp_policy_commands",
      "workspace_review_settings_commands",
    ],
    why: "Configuration-of-future-authority shapes cluster here: a persona/role/policy write does not act now but changes what a later spawn is allowed to do. This is the `update_custom_analysis` family of risk (§3.3 backstop-1 residual), so it gets one focused dual-lens review instead of being sprinkled across batches.",
    work: [
      "For each command ask the deferred-authority question explicitly: does this write change what a FUTURE agent process may do? If yes it is at least `AgentControl` with `ConfiguresFutureProcessAuthority`, regardless of how inert the immediate action looks.",
      "`delete_persona`-shaped members stay Denied (deletesEntity).",
    ],
    gate: "P-17 green; C-9 review recorded with the deferred-authority lens explicitly exercised.",
  },
  {
    id: "B7",
    title: "Artifacts, task context, notifications, app chrome",
    modules: [
      "artifact_commands",
      "task_context_commands",
      "notification_commands",
      "ui_commands",
      "update_channel_commands",
      "release_notes_commands",
    ],
    why: "The tail. Mixed reads and small writes; also the batch that must decide which names are genuinely CLIENT-LOCAL (updater channel, window/dock chrome) and therefore belong in `local-only-commands.ts` with an honest reason — the only batch expected to add local-only rows.",
    work: [
      "Split client-local from host-owned per command: `update_channel_commands` and parts of `ui_commands` are plausible `local-only` rows; artifacts and task context are host state and must register or be manifest-disposed.",
      "`get_task_context` and the prompt-builder reads are content-surface members (ledger-soundness round found 5 dropped worker content reads) — re-check the surface enumeration before assigning.",
      "Every local-only row gets an honest client-local reason; 'hard to classify' is never valid.",
    ],
    gate: "P-17 green; every new local-only row has a reason; C-9 review recorded.",
  },
  {
    id: "D1",
    retiredBy: "B0",
    title: "Credential + integration surface (disposition only, no registrations)",
    modules: [
      "ticketing_commands",
      "atlassian_commands",
      "linear_commands",
      "granola_commands",
      "clickup_commands",
      "api_key_commands",
      "external_mcp_commands",
      "harness_provider_commands",
    ],
    why: "Every member is `TouchesCredentials` or `ConfiguresFutureProcessAuthority`. API-key management is compile-denied from the facade (§4.3) and the integration-settings saves are the round-3 module deny list. Nothing here registers in v1; the entire batch is manifest disposition, so it is pure throughput once B0 lands — 72 names retired with zero registration risk.",
    work: [
      "Confirm each ledger row already carries the denying capability; add missing rows rather than adding local-only reasons.",
      "The ticketing reads are Elevated-not-Denied (they read a credentialed provider): decide once, for the whole module, whether v1 defers them or the reads split from the writes in a later phase. Record the decision in the ledger reason.",
    ],
    gate: "Manifest regenerated and diff-clean; unclassified count drops by exactly this batch's size; zero new local-only rows.",
  },
  {
    id: "D2",
    retiredBy: "B0",
    title: "Process-launch getters and git/gh surface (disposition only)",
    modules: [
      "diff_commands",
      "project_commands",
      "plan_branch_commands",
      "github_commands",
      "git_commands",
      "agent_issue_report_commands",
      "provider_cli_management_commands",
      "workspace_open_commands",
    ],
    why: "The 'getter that shells out' family plus the destructive-git and installer surfaces. `SpawnsProcess` is not exposable at any v1 scope, so these are dispositions, not registrations. `get_project`/`list_projects` are carved out into R1 because they are the one case where the spawn is removable rather than inherent.",
    work: [
      "Verify each row carries `SpawnsProcess` (detector (c) is the floor: a Read/Operate row reaching a launch sink fails CI).",
      "`get_task_file_changes` / `get_file_diff` / `get_codex_cli_diagnostics` are the named getter-spawns — they stay denied even though they read like reads.",
    ],
    gate: "Manifest diff-clean; detector-(c) floor test green; unclassified count drops by exactly this batch's size.",
  },
  {
    id: "R1",
    retiredBy: "B0",
    retiredNote:
      "NOT closed, though: leaving the ratchet is a bookkeeping fact, not an answer. Both names are manifest-classified `host-denied-spawns-process` because the getter shells out TODAY; §5.1's open question is whether to remove the spawn so they can be registered, and that owner call still stands. If it is answered yes, these rows change class and re-enter as registration work.",
    title: "`get_project` / `list_projects` — spawn-free read path",
    modules: [],
    commands: ["get_project", "list_projects"],
    why: "The only commands in the gap whose process authority is INCIDENTAL. Both are pure repository reads; the single spawning field is `repository_capability`, computed per project by shelling out to git in `project_response()`. Removing that inline shell-out makes the highest-traffic read on the whole remote surface registerable as `Read`. See `resolvedItems.projectGetters` for the proposed path and the rejected alternatives.",
    work: [
      "Land the cache-backed capability read (option A in `resolvedItems.projectGetters`) as its own change with its own tests — NOT inside a registration batch.",
      "Only after the shell-out is gone: re-run detector (c), drop the `SpawnsProcess` capability, reclassify to `Read`, register both.",
      "If option A is rejected by the owner, both names fall back into D2 as v1-deferred dispositions and the frontend project list stays a fetch-route question (3.1 open question 4).",
    ],
    gate: "Detector (c) reports no launch sink in either closure; P-4 parity rows for both; the manifest shows class `read` with an empty capability set.",
  },
  {
    id: "D3",
    retiredBy: "B0",
    title: "Host chrome, terminal, repository settings, test data (disposition only)",
    modules: [
      "startup_commands",
      "repository_settings_commands",
      "agent_terminal_commands",
      "test_data_commands",
    ],
    why: "Terminal is the phase doc's worked example of the third disposition: its invoke names resolve for P-11 through the module-`Denied` (`PtyControl`) rows, NEVER through a client-local reason. Test data is hard-denied outright (total-data-loss blast radius). Startup/repository settings are `HostManagement`/`ConfiguresFutureProcessAuthority` — v1-deferred.",
    work: [
      "Assert the terminal names resolve through the manifest path introduced in B0; a `local-only` row for any of them is a defect, not a shortcut.",
      "Keep `report_startup_frontend_milestone` honest: it is a client-originated report about the LOCAL app boot — check whether it is genuinely client-local (local-only row) rather than host-deferred.",
    ],
    gate: "Manifest diff-clean; a planted local-only row for a terminal command fails CI.",
  },
  {
    id: "A1",
    retiredBy: "B0",
    retiredNote:
      "NOT closed: the attachment names leave the ratchet, but remote attachment RENDERING is a fetch route, not an invoke command, and §5.3's `ChatAttachmentGallery.tsx` gap plus the 1.5-C endpoint dependency are untouched by B0.",
    title: "Chat attachments — disposition + remote rendering (deferred from 2.6/review-4)",
    modules: ["chat_attachment_commands"],
    why: "2.6-a shipped the honest interim: under a remote environment `getImagePreviewSrc()` returns `null` and every attachment renders as a placeholder card, because `convertFileSrc` mints an `asset://` URL for a path on the CLIENT's filesystem while `attachment.filePath` names a file on the HOST. The comment at `MessageAttachments.tsx:92-94` defers the real fix to 3.1. The three attachment commands are Denied (`writesArbitraryPath` / `deletesEntity`) and stay dispositions — the rendering work is a FETCH-path change, not a command registration, which is exactly 3.1 open question 4 and needs an explicit call.",
    work: [
      "Blocked on 1.5-C: `/remote/v1/attachments/{id}` does not exist on this base (no `attachments` route in `remote_server/`). Do not start A1 until the 1.5 lane lands it.",
      "Branch preview-source resolution on env kind in BOTH renderers — `MessageAttachments.tsx:115` and `ChatAttachmentGallery.tsx:97` (2.6 only hardened the first; the gallery still calls `convertFileSrc` unconditionally, which is a live gap this census surfaces).",
      "Route the remote branch through the scoped endpoint under `ui:read` with a binary-safe body and 2.7's response-header envelope; never through JSON `/invoke` (C-16).",
      "Resolve open question 4 explicitly: extending the §3.5 fetch-route remount allowlist rides 3.1, or it is a separate change against P-1's checked-in allowlist. Record the call.",
      "Disposition the three commands through the manifest (upload/delete are `writesArbitraryPath` fs sinks; `list_message_attachments` is denied by module).",
    ],
    gate: "A remote attachment renders through the scoped endpoint; a local one still uses `convertFileSrc`; P-1 route-allowlist equality still holds; the three commands are manifest-disposed with zero local-only rows.",
  },
  {
    id: "X1",
    title: "Orphan invokes — no local handler exists",
    modules: [],
    commands: [],
    why: "RESOLVED in PR 3.1-b. Five gap names were absent from `src-tauri/src/commands/registry.rs` `generate_handler!` AND from the ledger (which is exhaustive over it), so every call rejected at runtime with no remote environment involved. The reachability audit found none of the five was wired to any component or event handler — the wrappers were dead code kept alive only by their own unit tests — so all five were resolved by DELETING the call site rather than by minting host authority the product does not use. This batch is now empty and stays here as the record of that call.",
    work: [
      "`add_proposal_dependency` — deleted at both call sites (`api/ideation.ts` `dependencies.add`, `api/proposal.ts` `addProposalDependency`) plus the `addDependency` mutation in `hooks/useDependencyGraph.ts`. Its owning hook `useDependencyMutations` had no consumer at all: the UI reads the dependency graph and never writes edges, so there was no product asymmetry to fix by adding the missing command.",
      "`create_child_session` / `get_parent_session_context` — deleted from `api/ideation.ts` (zero callers, zero tests). The capability is not lost: both are live HTTP routes (`POST /api/create_child_session`, `GET /api/parent_session_context/:session_id`), which is how the backend actually reaches them.",
      "`delete_project` — deleted from `api/projects.ts`; `projectsApi.archive` is the live removal path.",
      "`delete_task` — deleted from `api/tasks.ts` and `hooks/useTaskMutation.ts`; it was already `@deprecated Use cleanupTask instead`, and every component destructures `cleanupTaskMutation`.",
      "Regression guard: `frontend/src/api/orphan-invokes.test.ts` asserts each wrapper stays absent while its surviving sibling (`remove`, `getChildren`, `archive`, `cleanupTask`) stays present, so the test cannot pass by the namespace disappearing.",
    ],
    gate: "Each of the five is deleted at the call site with a regression test; the P-11 scan sees zero orphans.",
  }
];

// ---------------------------------------------------------------------------
// Load + verify inputs
// ---------------------------------------------------------------------------

let scanOutput;
try {
  scanOutput = execFileSync("node", [scanPath, repoRoot], { encoding: "utf8" });
} catch (error) {
  fail(
    `the drift scan does not pass; the census refuses to describe a surface the scan rejects.\n${
      error.stdout ?? ""
    }${error.stderr ?? ""}`
  );
}

const scanMatch = scanOutput.match(
  /PASS: remote transport drift — (\d+) invoke command name\(s\), (\d+) dynamic, (\d+) seam bypasses; (\d+) manifest-classified; (\d+) unclassified/
);
if (!scanMatch) fail(`could not parse the drift-scan PASS line: ${scanOutput.trim()}`);

const scan = {
  invokedCommands: Number(scanMatch[1]),
  dynamicExpressions: Number(scanMatch[2]),
  seamBypasses: Number(scanMatch[3]),
  manifestClassified: Number(scanMatch[4]),
  unclassified: Number(scanMatch[5]),
  line: scanOutput.trim(),
};

const baseline = JSON.parse(fs.readFileSync(baselinePath, "utf8"));
const gap = [...(baseline.unclassifiedCommands ?? [])].sort();
if (gap.length !== scan.unclassified) {
  fail(
    `baseline holds ${gap.length} unclassified command(s) but the scan reports ${scan.unclassified}`
  );
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const ledger = new Map(manifest.ledger.map((entry) => [entry.command, entry]));
const registeredCount = manifest.ledger.filter((entry) => entry.registered).length;
const localOnlyCount = (fs.readFileSync(localOnlyPath, "utf8").match(/^\s*command:/gm) ?? [])
  .length;

// ---------------------------------------------------------------------------
// Classify + partition
// ---------------------------------------------------------------------------

const records = gap.map((command) => {
  const entry = ledger.get(command) ?? null;
  return {
    command,
    module: entry?.module ?? null,
    ledgerClass: entry?.class ?? null,
    capabilities: entry?.capabilities ?? [],
    ledgerReason: entry?.reason ?? null,
    disposition: dispositionOf(command, entry),
  };
});

const pinned = new Map();
for (const batch of BATCHES) {
  for (const command of batch.commands ?? []) {
    if (pinned.has(command)) fail(`command pinned to two batches: ${command}`);
    pinned.set(command, batch.id);
  }
}

const moduleOwner = new Map();
for (const batch of BATCHES) {
  for (const module of batch.modules ?? []) {
    if (moduleOwner.has(module)) fail(`module claimed by two batches: ${module}`);
    moduleOwner.set(module, batch.id);
  }
}

const assignment = new Map();
for (const record of records) {
  const pinnedBatch = pinned.get(record.command);
  const moduleBatch = record.module ? moduleOwner.get(record.module) : undefined;
  const batchId = pinnedBatch ?? moduleBatch;
  if (!batchId) {
    fail(
      `unassigned command ${record.command} (module ${record.module ?? "<none>"}) — every gap command must land in exactly one batch`
    );
  }
  assignment.set(record.command, batchId);
  record.batch = batchId;
}

// A batch is RETIRED when B0's manifest classification took its whole membership out of the
// gap — the disposition-only batches exist precisely to be retired this way. The check is
// two-directional so neither half can rot: a live batch must still have gap members, and a
// batch marked retired must have NONE. Marking a batch retired to silence a staleness failure
// therefore fails immediately if any member is still unclassified.
const batchIsEmpty = (batch) =>
  !(batch.modules ?? []).some((module) =>
    records.some((record) => record.module === module)
  ) && !(batch.commands ?? []).some((command) =>
    records.some((record) => record.command === command)
  );

for (const batch of BATCHES) {
  const empty = batchIsEmpty(batch);
  if (batch.retiredBy && !empty) {
    fail(
      `batch ${batch.id} is marked retired by ${batch.retiredBy}, but some of its members are still unclassified`
    );
  }
  if (!batch.retiredBy && empty && (batch.modules?.length || batch.commands?.length)) {
    fail(
      `batch ${batch.id} claims modules/commands that have no unclassified commands — mark it retiredBy if a mechanism batch resolved them`
    );
  }
}

for (const [module, batchId] of moduleOwner) {
  const batch = BATCHES.find((candidate) => candidate.id === batchId);
  if (batch?.retiredBy) continue;
  if (!records.some((record) => record.module === module)) {
    fail(`batch ${batchId} claims module ${module}, which has no unclassified commands`);
  }
}
for (const [command, batchId] of pinned) {
  const batch = BATCHES.find((candidate) => candidate.id === batchId);
  if (batch?.retiredBy) continue;
  if (!records.some((record) => record.command === command)) {
    fail(`batch ${batchId} pins ${command}, which is not in the gap`);
  }
}

const dispositionTotals = {};
for (const record of records) {
  dispositionTotals[record.disposition] = (dispositionTotals[record.disposition] ?? 0) + 1;
}

const batches = BATCHES.map((batch, index) => {
  const members = records
    .filter((record) => record.batch === batch.id)
    .sort((a, b) => a.command.localeCompare(b.command));
  const byDisposition = {};
  const byModule = {};
  for (const member of members) {
    byDisposition[member.disposition] = (byDisposition[member.disposition] ?? 0) + 1;
    const key = member.module ?? "<no ledger entry>";
    (byModule[key] ??= []).push(member.command);
  }
  return {
    id: batch.id,
    order: index + 1,
    title: batch.title,
    rationale: batch.why,
    retiredBy: batch.retiredBy ?? null,
    retiredNote: batch.retiredNote ?? null,
    work: batch.work,
    gate: batch.gate,
    commandCount: members.length,
    modules: Object.keys(byModule).sort(),
    dispositionCounts: byDisposition,
    registerCandidates: byDisposition.registerCandidate ?? 0,
    nonRegistering: members.length - (byDisposition.registerCandidate ?? 0),
    commandsByModule: Object.fromEntries(
      Object.entries(byModule).sort(([a], [b]) => a.localeCompare(b))
    ),
    commands: members.map((member) => member.command),
  };
});

const assigned = batches.reduce((sum, batch) => sum + batch.commandCount, 0);
if (assigned !== gap.length) {
  fail(`batches cover ${assigned} command(s) but the gap holds ${gap.length}`);
}

// ---------------------------------------------------------------------------
// Resolved items (the three named 3.1-a questions)
// ---------------------------------------------------------------------------

const RESOLVED_ITEMS = {
  projectGetters: {
    commands: ["get_project", "list_projects"],
    batch: "R1",
    status: "proposal — needs an owner call before 3.1-b starts R1",
    finding:
      "Both are pure repository reads (`project_commands.rs:211-240`): `project_repo.get_all()` / `get_by_id()`, then `project_response()` per row. The ONLY process authority is one response field — `repository_capability`, produced by `inspect_repository_capability()` (`infrastructure/git_auth.rs`), which runs `git remote get-url origin` and `git remote get-url --push origin` through `resolve_git_cli_path()` with a 5s deadline, once PER PROJECT. That is what makes a getter a `SpawnsProcess`/Elevated command, and it is incidental to the read, not inherent to it.",
    proposal:
      "Option A — cache the capability, do not compute it in the getter. Persist the inspected `repository_capability` (plus `inspected_at`) alongside the project row, write it from the paths that already have process authority and already shell out (project create/update, `change_project_git_mode`, `setup_gh_git_auth`, `switch_git_origin_to_ssh`, `reanalyze_project`) plus one background refresh whose loop root is declared in the manifest's `background_loop_inventory`, and have `project_response()` READ the cached value. `list_projects`/`get_project` then hold no launch sink in their closure, detector (c) goes quiet, the `SpawnsProcess` capability drops, and both classify as `Read` — registerable on the v1 facade at `ui:read`, with zero `generate_handler!` edits and zero command-fn forks (A-7). The response shape is unchanged, so P-4 parity and every existing caller are untouched; only the freshness semantics change, and a stale-capability value is strictly safer than the current InspectionFailed-on-timeout behaviour (`inspect_repository_capability` already returns `InspectionFailed{message}` rather than erroring, so consumers already handle a non-authoritative value).",
    rejectedAlternatives: [
      "A response projection that omits `repository_capability` for remote callers — that is a command-fn fork, which A-7 forbids, and it would break P-4 byte-identity between local IPC and remote dispatch (the whole point of the parity suite).",
      "A pinned facade op — pins fix ARGUMENTS (`approve_permission_request` / `deny_permission_request`), not response shape; there is no pin that removes a field.",
      "Registering as Elevated — `ui:elevated` is a §1 v1 non-goal; this would ship a scope nothing can hold.",
      "Serving the project list over a remounted fetch route instead — `http_server/handlers/projects.rs` computes the SAME capability inline, so the spawn moves rather than disappears, and it opens 3.1 open question 4 unnecessarily.",
    ],
    ifRejected:
      "Both names fall back to D2 as v1-deferred dispositions. That is not cost-free: the project list is the entry point of nearly every remote screen, so a remote client would have to hydrate projects through a fetch route (open question 4) or run with no project list at all.",
  },
  unregisteredUiAgentOps: {
    commands: [
      "send_agent_message",
      "start_agent_conversation",
      "skip_step",
      "trigger_automation_run_now",
      "restart_automation",
    ],
    batches: { send_agent_message: "B2", start_agent_conversation: "B2", skip_step: "B1", trigger_automation_run_now: "B5", restart_automation: "B5" },
    status: "resolved — all five are registration candidates; none is a detector-(c) rejection on current evidence",
    briefingCorrection:
      "The 3.1-a brief states three of these five are detector-(c)-rejected. That does not match the code: the detector-(c) trio is `resume_task`, `apply_proposals_to_kanban`, `set_agent_conversation_workspace_auto_publish` (`remote_server/registry.rs` NOT-registered note; `frontend/src/lib/remote/agent-gate.test.ts:114-124` uses exactly those three as the unavailable-by-ABSENCE fixture). None of the five 2.6-surfaced ops appears in that set. The two lists were conflated — they are different trios, and 2.6's tracker note lists the five as ops that 'flip with no client change when 3.1 registers them', i.e. registration is the intended resolution.",
    evidence: [
      "2.6 tracker product note: 'with `ui:agent` granted, chat send / start composer / skip_step / automation run+restart render UNAVAILABLE remotely — send_agent_message etc. are unregistered in 1.5-A's 27-op surface. Honest against this build; flips with no client change when 3.1 registers them.'",
      "Phase 3 doc, PR 3.2 key point 4: 'Chat send paths (`start_agent_conversation`, `send_agent_message` + variants, …) are `AgentControl` — a device without `ui:agent` gets `REMOTE_FORBIDDEN`'. `REMOTE_FORBIDDEN` (not `REMOTE_COMMAND_UNAVAILABLE`) is only reachable for a REGISTERED command, so 3.2 requires these registered.",
      "All five are ledgered `class: agentControl`, `capabilities: [agentControl]`, reason `conservative-module-default` — none carries `SpawnsProcess`.",
      "`send_agent_message` reaches `chat_service.send_message` (`unified_chat_commands/mod.rs`), which is a detector-(a) STEER sink. `all_cut_sinks()` CUTS the closure at steer sinks, so the provider process launch beyond it is outside the command's own closure — which is precisely why chat send is registerable while `resume_task` (whose closure resolves a CLI path directly) is not.",
    ],
    obligation:
      "This is a static read of the call graph, not a detector run. 3.1-b must confirm each of the five against the live P-17 detector-(c) output as the first step of its batch, and demote any that come back positive to a manifest disposition — the class is decided by the detector, never by this census.",
    clientImpact:
      "No client change is needed: `agent-gate.ts` derives availability from ABSENCE in `facade_ops`, so each op flips from `unavailable` to `gated`/`enabled` the moment its registration lands in the regenerated manifest.",
  },
  remoteAttachmentRendering: {
    batch: "A1",
    status: "scoped into batch A1; BLOCKED on the 1.5-C endpoint",
    finding:
      "Deferred here from 2.6-a and the review-4 round. Current behaviour is the honest interim, not a bug: `getImagePreviewSrc()` (`frontend/src/components/Chat/MessageAttachments.tsx:99-116`) returns `null` whenever the active environment is remote, so every host attachment renders as a placeholder card instead of a broken image — `convertFileSrc` would mint an `asset://` URL for a path on the CLIENT's disk while `attachment.filePath` names a file on the HOST.",
    blockers: [
      "`/remote/v1/attachments/{id}` does not exist on this base — there is no attachments route in `src-tauri/src/remote_server/`. It is 1.5-C's deliverable (live in the `rme-pr-1-5` lane). A1 cannot start until it lands.",
      "2.7's response-header envelope and a binary-safe body are prerequisites; binary must never travel through JSON `/invoke` (C-16).",
    ],
    newGapFoundByThisCensus:
      "2.6 hardened only ONE of the two renderers. `ChatAttachmentGallery.tsx:97` still calls `convertFileSrc(attachment.filePath)` with no env-kind branch, so the gallery surface renders broken images under a remote environment where `MessageAttachments` renders placeholders. A1 must fix both, and the 2.6 negative test (`host-affordance-gating.test.tsx`, which asserts `convertFileSrc` was NOT called) should be extended to cover the gallery.",
    openQuestion:
      "Phase-3 open question 4 applies verbatim: attachment rendering is a FETCH route, not an invoke command, and the source does not say whether extending the §3.5 remount allowlist rides 3.1 or requires a separate change against P-1's checked-in allowlist. A1 must record the call before it writes a route.",
    commandSide:
      "The three attachment COMMANDS in the gap (`upload_chat_attachment`, `delete_chat_attachment`, `list_message_attachments`) are all ledgered `denied` (`writesArbitraryPath` / `deletesEntity`) and stay manifest dispositions — no registration, and specifically no local-only rows.",
  },
};

// ---------------------------------------------------------------------------
// Emit
// ---------------------------------------------------------------------------

const census = {
  schemaVersion: 1,
  generatedBy: "scripts/generate-remote-coverage-census.mjs",
  purpose:
    "PR 3.1 work manifest: every P-11-unclassified invoke command name, grouped into module-scoped work batches with a recommended registration order. A census and a plan — it registers nothing and decides no final class.",
  scan,
  totals: {
    invokedCommandNames: scan.invokedCommands,
    remoteRegistered: registeredCount,
    localOnlyRows: localOnlyCount,
    ledgerRows: manifest.ledger.length,
    unclassified: gap.length,
    assignedToBatches: assigned,
    batches: batches.length,
  },
  dispositionModel: DISPOSITIONS,
  dispositionTotals,
  batches,
  commands: records.map((record) => ({
    command: record.command,
    batch: record.batch,
    module: record.module,
    ledgerClass: record.ledgerClass,
    capabilities: record.capabilities,
    disposition: record.disposition,
    ledgerReason: record.ledgerReason,
  })),
  resolvedItems: RESOLVED_ITEMS,
};

function mdTable(headers, rows) {
  return [
    `| ${headers.join(" | ")} |`,
    `|${headers.map(() => "---").join("|")}|`,
    ...rows.map((row) => `| ${row.join(" | ")} |`),
  ].join("\n");
}

function renderMarkdown(data) {
  const d = data.dispositionTotals;
  const lines = [];
  lines.push("# PR 3.1 — Facade coverage census (P-11 gap work manifest)");
  lines.push("");
  lines.push(
    "> GENERATED — do not edit by hand. Regenerate: `node scripts/generate-remote-coverage-census.mjs`. Staleness gate: `--check`."
  );
  lines.push(
    "> This is the PR 3.1-a planning artifact. It registers nothing. Every class here is the ledger's CURRENT value; the per-command hand audit (§3.3) and the P-17 detector run own the final one."
  );
  lines.push("");
  lines.push("## 1. Scan state");
  lines.push("");
  lines.push("```");
  lines.push(data.scan.line);
  lines.push("```");
  lines.push("");
  lines.push(
    mdTable(
      ["Measure", "Count", "Source"],
      [
        ["Invoke command names in `frontend/src`", data.totals.invokedCommandNames, "drift scan (AST)"],
        ["Dynamic / unresolvable expressions", data.scan.dynamicExpressions, "drift scan — must stay 0"],
        ["Transport seam bypasses", data.scan.seamBypasses, "drift scan — must stay 0"],
        ["Remote-registered (`remote_commands!`)", data.totals.remoteRegistered, "`docs/generated/remote-commands.json`"],
        ["Reason-coded local-only rows", data.totals.localOnlyRows, "`frontend/src/lib/remote/local-only-commands.ts`"],
        ["Ledger rows (exhaustive over `generate_handler!`)", data.totals.ledgerRows, "`docs/generated/remote-commands.json`"],
        ["Manifest-classified (host-denied / v1-deferred)", data.scan.manifestClassified, "`v1Resolution` in `docs/generated/remote-commands.json`"],
        ["**Unclassified — the 3.1 gap**", `**${data.totals.unclassified}**`, "`scripts/remote-transport-drift-baseline.json`"],
      ]
    )
  );
  lines.push("");
  lines.push("## 2. What the gap is made of");
  lines.push("");
  lines.push(
    "Routing each name mechanically through the ledger splits it into very different kinds of work. B0 has already retired the three non-registerable dispositions from the gap, so they read 0 here — their members now resolve through the manifest and no longer sit in the baseline:"
  );
  lines.push("");
  lines.push(
    mdTable(
      ["Disposition", "Count", "Rule"],
      Object.entries(DISPOSITIONS).map(([key, value]) => [
        value.label,
        d[key] ?? 0,
        value.rule,
      ])
    )
  );
  lines.push("");
  lines.push(
    `**${data.scan.manifestClassified} invoked names now resolve through the manifest** — host-side commands the facade denies or defers, classified by their ledger row's \`v1Resolution\` rather than by a registration or a client-local reason (phase-doc key point 6). B0 landed that mechanism and the gap fell 419 → ${data.totals.unclassified} with zero registrations. **What remains in the baseline is registration work only**, so from here every batch's delta is exactly the count it registers.`
  );
  lines.push("");
  lines.push(
    `**${d.registerCandidate ?? 0} names are registration candidates**, and \`register-candidate\` means eligible for a hand audit, not approved: detector (c) has already rejected ledgered-\`AgentControl\` commands whose process authority the manifest cannot see (\`resume_task\`, \`apply_proposals_to_kanban\`, \`set_agent_conversation_workspace_auto_publish\`). Expect a non-empty rejection subset in every registration batch.`
  );
  lines.push("");
  lines.push("## 3. Recommended batch order");
  lines.push("");
  lines.push(
    mdTable(
      ["#", "Batch", "Title", "Cmds", "Register-candidates", "Not registering", "Modules"],
      data.batches.map((batch) => [
        batch.order,
        `\`${batch.id}\``,
        batch.title,
        batch.commandCount,
        batch.registerCandidates,
        batch.nonRegistering,
        batch.modules.length,
      ])
    )
  );
  lines.push("");
  lines.push(
    "Ordering logic: **B0 first** (nothing is measurable without the third disposition) → **B1** (smallest parity risk, reuses 1.5-A's proven injection shapes) → **B2** (unblocks PR 3.2, which cannot start until chat send answers `REMOTE_FORBIDDEN` instead of `REMOTE_COMMAND_UNAVAILABLE`) → **B3–B7** registration batches by falling audit risk → **D1/D2/D3** disposition-only batches, which retire large blocks with zero registration risk and can run in parallel with any registration batch once B0 lands → **R1** (a code change, not a registration, and gated on an owner call) → **A1** (blocked on 1.5-C) → **X1** (live defects, independent of remote work)."
  );
  lines.push("");
  lines.push("## 4. Batches");
  for (const batch of data.batches) {
    lines.push("");
    lines.push(`### ${batch.order}. \`${batch.id}\` — ${batch.title}`);
    lines.push("");
    lines.push(
      `**Commands:** ${batch.commandCount} · **Register-candidates:** ${batch.registerCandidates} · **Risk classes:** ${
        Object.entries(batch.dispositionCounts)
          .map(([key, count]) => `${DISPOSITIONS[key].label} ${count}`)
          .join(" · ") || "—"
      }`
    );
    lines.push("");
    if (batch.retiredBy) {
      lines.push(
        `**Retired by \`${batch.retiredBy}\`.** Every member left the P-11 ratchet as manifest-classified, so this batch has no registration work. ${
          batch.retiredNote ??
          "Disposition-only from the start — the manifest classification IS the disposition."
        }`
      );
      lines.push("");
    }
    lines.push(`**Why here:** ${batch.rationale}`);
    lines.push("");
    if (batch.work.length > 0) {
      lines.push("**Work:**");
      lines.push("");
      for (const item of batch.work) lines.push(`- ${item}`);
      lines.push("");
    }
    lines.push(`**Gate:** ${batch.gate}`);
    if (batch.commandCount > 0) {
      lines.push("");
      lines.push("<details><summary>Members by module</summary>");
      lines.push("");
      for (const [module, commands] of Object.entries(batch.commandsByModule)) {
        lines.push(`- **\`${module}\`** (${commands.length}) — ${commands.map((c) => `\`${c}\``).join(", ")}`);
      }
      lines.push("");
      lines.push("</details>");
    }
  }
  lines.push("");
  lines.push("## 5. Resolved items");
  lines.push("");
  lines.push("### 5.1 `get_project` / `list_projects` — the getter that shells out");
  lines.push("");
  lines.push(`**Status:** ${RESOLVED_ITEMS.projectGetters.status} · **Batch:** \`R1\``);
  lines.push("");
  lines.push(`**Finding.** ${RESOLVED_ITEMS.projectGetters.finding}`);
  lines.push("");
  lines.push(`**Proposal.** ${RESOLVED_ITEMS.projectGetters.proposal}`);
  lines.push("");
  lines.push("**Rejected alternatives:**");
  lines.push("");
  for (const item of RESOLVED_ITEMS.projectGetters.rejectedAlternatives) lines.push(`- ${item}`);
  lines.push("");
  lines.push(`**If the owner rejects option A.** ${RESOLVED_ITEMS.projectGetters.ifRejected}`);
  lines.push("");
  lines.push("### 5.2 The five 2.6-surfaced unregistered `ui:agent` ops");
  lines.push("");
  lines.push(`**Status:** ${RESOLVED_ITEMS.unregisteredUiAgentOps.status}`);
  lines.push("");
  lines.push(
    mdTable(
      ["Command", "Ledger class", "Capabilities", "Batch", "Resolution"],
      RESOLVED_ITEMS.unregisteredUiAgentOps.commands.map((command) => {
        const record = data.commands.find((item) => item.command === command);
        return [
          `\`${command}\``,
          record.ledgerClass,
          record.capabilities.join(", ") || "—",
          `\`${RESOLVED_ITEMS.unregisteredUiAgentOps.batches[command]}\``,
          "register (`ui:agent`), pending detector-(c) confirmation",
        ];
      })
    )
  );
  lines.push("");
  lines.push(`**Briefing correction.** ${RESOLVED_ITEMS.unregisteredUiAgentOps.briefingCorrection}`);
  lines.push("");
  lines.push("**Evidence:**");
  lines.push("");
  for (const item of RESOLVED_ITEMS.unregisteredUiAgentOps.evidence) lines.push(`- ${item}`);
  lines.push("");
  lines.push(`**Obligation on 3.1-b.** ${RESOLVED_ITEMS.unregisteredUiAgentOps.obligation}`);
  lines.push("");
  lines.push(`**Client impact.** ${RESOLVED_ITEMS.unregisteredUiAgentOps.clientImpact}`);
  lines.push("");
  lines.push("### 5.3 Remote attachment rendering");
  lines.push("");
  lines.push(`**Status:** ${RESOLVED_ITEMS.remoteAttachmentRendering.status} · **Batch:** \`A1\``);
  lines.push("");
  lines.push(`**Finding.** ${RESOLVED_ITEMS.remoteAttachmentRendering.finding}`);
  lines.push("");
  lines.push("**Blockers:**");
  lines.push("");
  for (const item of RESOLVED_ITEMS.remoteAttachmentRendering.blockers) lines.push(`- ${item}`);
  lines.push("");
  lines.push(`**New gap this census found.** ${RESOLVED_ITEMS.remoteAttachmentRendering.newGapFoundByThisCensus}`);
  lines.push("");
  lines.push(`**Open question.** ${RESOLVED_ITEMS.remoteAttachmentRendering.openQuestion}`);
  lines.push("");
  lines.push(`**Command side.** ${RESOLVED_ITEMS.remoteAttachmentRendering.commandSide}`);
  lines.push("");
  lines.push("## 6. Reconciliation");
  lines.push("");
  lines.push(
    mdTable(
      ["Check", "Result"],
      [
        ["Drift scan passes", "yes (this file is not emitted otherwise)"],
        [
          "Scan unclassified count == baseline size",
          `${data.scan.unclassified} == ${data.totals.unclassified}`,
        ],
        [
          "Every gap command in exactly one batch",
          `${data.totals.assignedToBatches} / ${data.totals.unclassified}`,
        ],
        [
          "Disposition totals sum to the gap",
          `${Object.values(data.dispositionTotals).reduce((a, b) => a + b, 0)} == ${data.totals.unclassified}`,
        ],
        ["Batch plan claims no empty module and pins no absent command", "enforced by the generator"],
      ]
    )
  );
  lines.push("");
  lines.push(
    "Machine-readable companion for 3.1-b/c: [`remote-coverage-census.json`](./remote-coverage-census.json) — same batches, plus per-command `{batch, module, ledgerClass, capabilities, disposition}` rows."
  );
  lines.push("");
  return lines.join("\n");
}

const jsonText = `${JSON.stringify(census, null, 2)}\n`;
const mdText = renderMarkdown(census);

if (checkOnly) {
  const stale = [];
  for (const [file, expected] of [
    [jsonOutPath, jsonText],
    [mdOutPath, mdText],
  ]) {
    const actual = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : null;
    if (actual !== expected) stale.push(path.relative(repoRoot, file));
  }
  if (stale.length > 0) {
    console.error(
      `FAIL: remote coverage census is stale: ${stale.join(", ")} — regenerate with \`node scripts/generate-remote-coverage-census.mjs\``
    );
    process.exit(1);
  }
  console.log(
    `PASS: remote coverage census up to date — ${census.totals.unclassified} unclassified command(s) across ${census.batches.length} batch(es).`
  );
  process.exit(0);
}

fs.mkdirSync(path.dirname(mdOutPath), { recursive: true });
fs.writeFileSync(jsonOutPath, jsonText);
fs.writeFileSync(mdOutPath, mdText);
console.log(
  `Wrote ${path.relative(repoRoot, mdOutPath)} and ${path.relative(repoRoot, jsonOutPath)}: ` +
    `${census.totals.unclassified} unclassified command(s) across ${census.batches.length} batch(es) ` +
    `(${census.dispositionTotals.registerCandidate ?? 0} register-candidates).`
);
