import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
const PROMPTS = [
    "agents/ralphx-automation-setup/claude/prompt.md",
    "agents/ralphx-automation-setup/codex/prompt.md",
];
const CALLER_BOUND_TOOLS = [
    "run_automation_now",
    "pause_automation",
    "resume_automation",
    "cancel_automation_run",
    "cancel_automation",
    "restart_automation",
    "retry_automation_judge",
    "retry_automation_plan_judge",
    "skip_automation_judge",
    "get_automation_publish_status",
    "check_automation_publish_readiness",
    "update_automation_from_base",
    "publish_automation_workspace",
];
function readPrompt(relativePath) {
    return readFileSync(new URL(`../../../../../${relativePath}`, import.meta.url), "utf8");
}
describe("automation setup prompt contract", () => {
    it.each(PROMPTS)("documents the live caller-bound tools in %s", (relativePath) => {
        const prompt = readPrompt(relativePath);
        for (const tool of CALLER_BOUND_TOOLS) {
            expect(prompt, `${relativePath} should mention ${tool}`).toContain(`\`${tool}\``);
        }
        expect(prompt).toContain("Do not ask for, infer, or send an automation id or conversation id");
        expect(prompt).toContain("An expired attempt that is still `in_progress` is not retryable until RalphX records it as `failed`");
        expect(prompt).not.toContain("failed/expired");
        expect(prompt).not.toContain("Do not edit files, run shell commands, publish branches, create agent workspaces, activate runs, or trigger runs.");
    });
});
//# sourceMappingURL=automation-prompt-contract.test.js.map