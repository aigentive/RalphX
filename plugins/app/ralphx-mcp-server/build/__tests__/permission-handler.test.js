import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { handlePermissionRequest, normalizePermissionToolInput, shouldAutoApprovePermission, } from "../permission-handler.js";
describe("handlePermissionRequest runtime identity", () => {
    afterEach(() => {
        vi.unstubAllGlobals();
    });
    it("attaches only the injected conversation identity to the backend request", async () => {
        const fetchMock = vi
            .fn()
            .mockResolvedValueOnce(new Response(JSON.stringify({ request_id: "permission-1" }), { status: 200 }))
            .mockResolvedValueOnce(new Response(JSON.stringify({ decision: "deny" }), { status: 200 }));
        vi.stubGlobal("fetch", fetchMock);
        await handlePermissionRequest({
            tool_name: "Bash",
            tool_input: { command: "git push" },
            conversation_id: "conversation-spoofed",
        }, { conversationId: "conversation-current" });
        expect(fetchMock).toHaveBeenNthCalledWith(1, "http://127.0.0.1:3847/api/permission/request", expect.objectContaining({
            headers: expect.objectContaining({
                "x-ralphx-conversation-id": "conversation-current",
            }),
        }));
        const requestBody = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body));
        expect(requestBody).not.toHaveProperty("conversation_id");
    });
});
describe("normalizePermissionToolInput", () => {
    it("adds snake_case and path aliases for Write requests", () => {
        expect(normalizePermissionToolInput("Write", {
            filePath: "/tmp/out.md",
            content: "hello",
        })).toEqual({
            filePath: "/tmp/out.md",
            file_path: "/tmp/out.md",
            path: "/tmp/out.md",
            content: "hello",
        });
    });
    it("maps Read path into file_path aliases", () => {
        expect(normalizePermissionToolInput("Read", {
            path: "/tmp/input.md",
        })).toEqual({
            path: "/tmp/input.md",
            file_path: "/tmp/input.md",
            filePath: "/tmp/input.md",
        });
    });
    it("adds snake_case aliases for Edit camelCase fields", () => {
        expect(normalizePermissionToolInput("Edit", {
            filePath: "/tmp/file.ts",
            oldString: "before",
            newString: "after",
        })).toEqual({
            filePath: "/tmp/file.ts",
            file_path: "/tmp/file.ts",
            path: "/tmp/file.ts",
            oldString: "before",
            old_string: "before",
            newString: "after",
            new_string: "after",
        });
    });
});
describe("shouldAutoApprovePermission", () => {
    const tempDirs = [];
    const originalPwd = process.env.PWD;
    const originalFilesystemReadRoots = process.env.RALPHX_FILESYSTEM_READ_ROOTS;
    afterEach(() => {
        process.env.PWD = originalPwd;
        if (originalFilesystemReadRoots === undefined) {
            delete process.env.RALPHX_FILESYSTEM_READ_ROOTS;
        }
        else {
            process.env.RALPHX_FILESYSTEM_READ_ROOTS = originalFilesystemReadRoots;
        }
        for (const dir of tempDirs.splice(0)) {
            fs.rmSync(dir, { recursive: true, force: true });
        }
        vi.restoreAllMocks();
    });
    function makeTempGitRepo() {
        const dir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-perm-"));
        tempDirs.push(dir);
        fs.mkdirSync(path.join(dir, ".git"));
        fs.writeFileSync(path.join(dir, "package.json"), "{}\n");
        return dir;
    }
    it("auto-approves Read for files inside a git repo", () => {
        const repo = makeTempGitRepo();
        const file = path.join(repo, "package.json");
        process.env.PWD = repo;
        expect(shouldAutoApprovePermission("Read", {
            path: file,
        })).toBe(true);
    });
    it("does not auto-approve Read for .env files even inside a git repo", () => {
        const repo = makeTempGitRepo();
        const file = path.join(repo, ".env");
        expect(shouldAutoApprovePermission("Read", {
            path: file,
        })).toBe(false);
    });
    it("auto-approves Glob for patterns rooted inside a git repo", () => {
        const repo = makeTempGitRepo();
        process.env.PWD = repo;
        expect(shouldAutoApprovePermission("Glob", {
            pattern: `${repo}/**/*.{ts,js,py}`,
        })).toBe(true);
    });
    it("auto-approves Grep when its target path is inside a git repo", () => {
        const repo = makeTempGitRepo();
        process.env.PWD = repo;
        expect(shouldAutoApprovePermission("Grep", {
            pattern: "permission",
            path: repo,
        })).toBe(true);
    });
    it("auto-approves read-only repo inspection Bash commands", () => {
        const repo = makeTempGitRepo();
        process.env.PWD = repo;
        expect(shouldAutoApprovePermission("Bash", {
            command: `cat ${path.join(repo, "package.json")} 2>/dev/null || echo 'FILE NOT FOUND'; ls -la ${repo}/`,
        })).toBe(true);
    });
    it("does not auto-approve mutating Bash commands", () => {
        const repo = makeTempGitRepo();
        process.env.PWD = repo;
        expect(shouldAutoApprovePermission("Bash", {
            command: `rm -rf ${repo}`,
        })).toBe(false);
    });
    it("auto-approves Read for ~/.reefagent/agents content", () => {
        const homeDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-home-"));
        tempDirs.push(homeDir);
        vi.spyOn(os, "homedir").mockReturnValue(homeDir);
        const agentDir = path.join(homeDir, ".reefagent", "agents", "spanish-tutor");
        fs.mkdirSync(agentDir, { recursive: true });
        const file = path.join(agentDir, "memory.md");
        fs.writeFileSync(file, "hola\n");
        expect(shouldAutoApprovePermission("Read", {
            path: file,
        })).toBe(true);
    });
    it("auto-approves Write for Claude project memory markdown files", () => {
        const homeDir = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-home-"));
        tempDirs.push(homeDir);
        vi.spyOn(os, "homedir").mockReturnValue(homeDir);
        const memoryDir = path.join(homeDir, ".claude", "projects", "sample-project", "memory");
        fs.mkdirSync(memoryDir, { recursive: true });
        const file = path.join(memoryDir, "feedback_telegram_response_style.md");
        expect(shouldAutoApprovePermission("Write", {
            filePath: file,
            content: "---\nname: test\n---\n",
        })).toBe(true);
    });
    it("does not auto-approve Write outside Claude project memory", () => {
        const repo = makeTempGitRepo();
        expect(shouldAutoApprovePermission("Write", {
            filePath: path.join(repo, "feedback.md"),
            content: "---\nname: test\n---\n",
        })).toBe(false);
    });
    it("does not auto-approve Read for a different git repo outside trusted roots", () => {
        const trustedRepo = makeTempGitRepo();
        const otherRepo = makeTempGitRepo();
        process.env.PWD = trustedRepo;
        expect(shouldAutoApprovePermission("Read", {
            path: path.join(otherRepo, "package.json"),
        })).toBe(false);
    });
    it("auto-approves read tools under a realpath-resolved configured read root", () => {
        const trustedRepo = makeTempGitRepo();
        const readRoot = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-read-root-")));
        tempDirs.push(readRoot);
        process.env.PWD = trustedRepo;
        const rootLink = path.join(trustedRepo, "configured-root-link");
        fs.symlinkSync(readRoot, rootLink);
        process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([rootLink]);
        const target = path.join(readRoot, "context.txt");
        fs.writeFileSync(target, "context\n");
        expect(shouldAutoApprovePermission("Read", { path: target })).toBe(true);
        expect(shouldAutoApprovePermission("LS", { path: readRoot })).toBe(true);
        expect(shouldAutoApprovePermission("Grep", { pattern: "context", path: readRoot })).toBe(true);
        expect(shouldAutoApprovePermission("Glob", { pattern: `${readRoot}/**/*.txt` })).toBe(true);
    });
    it("does not extend configured read-root trust to Bash", () => {
        const trustedRepo = makeTempGitRepo();
        const readRoot = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-read-root-")));
        tempDirs.push(readRoot);
        process.env.PWD = trustedRepo;
        process.env.RALPHX_FILESYSTEM_READ_ROOTS = JSON.stringify([readRoot]);
        const target = path.join(readRoot, "context.txt");
        fs.writeFileSync(target, "context\n");
        expect(shouldAutoApprovePermission("Read", { path: target })).toBe(true);
        expect(shouldAutoApprovePermission("Bash", { command: `cat ${target}` })).toBe(false);
    });
});
//# sourceMappingURL=permission-handler.test.js.map