/**
 * Composite: accept_plan_and_schedule
 *
 * Saga pattern — idempotent steps, resumable on failure.
 * Phase 4 implementation.
 *
 * Steps:
 * 1. Load ideation session + proposals
 * 2. Apply proposals (create task proposals in DB)
 * 3. Create tasks from accepted proposals
 * 4. Schedule tasks (set to pending, validate dependencies)
 *
 * On partial failure: returns progress report. Each step is idempotent.
 * Resume via v1_resume_scheduling tool.
 */
import type { ApiKeyContext } from "../types.js";
export interface AcceptAndScheduleInput {
    sessionId: string;
    baseBranchOverride?: string;
}
export interface AcceptAndScheduleProgress {
    step: "load_session" | "apply_proposals" | "create_tasks" | "schedule_tasks";
    completed: string[];
    failed?: {
        step: string;
        error: string;
    };
}
export interface AcceptAndScheduleResult {
    success: boolean;
    taskIds: string[];
    progress: AcceptAndScheduleProgress;
    verification?: {
        status: "queued" | "in_progress";
        sessionId: string;
        pollTool: "v1_get_plan_verification";
        retryTool: "v1_accept_plan_and_schedule";
    };
}
/**
 * Accept all proposals for a session and schedule the resulting tasks.
 * Each step is recorded so the saga can be resumed on failure.
 */
export declare function acceptAndSchedule(input: AcceptAndScheduleInput, context: ApiKeyContext): Promise<AcceptAndScheduleResult>;
//# sourceMappingURL=accept-and-schedule.d.ts.map