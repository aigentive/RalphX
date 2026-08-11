/**
 * Tests for secret redaction — mirrors the Rust secret_redactor test patterns.
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, it, expect, vi } from "vitest";
import {
  ARG_REDACTED_TOOLS,
  getTraceLogPath,
  redactToolArgsForLog,
  redactToolResultForLog,
  redactSecrets,
  resetTraceLogPathForTests,
  safeError,
  safeTrace,
} from "../redact.js";

afterEach(() => {
  delete process.env.RALPHX_MCP_TRACE_DIR;
  delete process.env.RALPHX_AGENT_TYPE;
  delete process.env.RALPHX_CONTEXT_TYPE;
  delete process.env.RALPHX_CONTEXT_ID;
  delete process.env.RALPHX_TASK_ID;
  delete process.env.RALPHX_PROJECT_ID;
  delete process.env.RALPHX_WORKING_DIRECTORY;
  resetTraceLogPathForTests();
});

describe("persona tool argument redaction", () => {
  it.each(["save_persona_draft", "get_persona_draft", "persona_future_tool"])(
    "redacts %s arguments before logging",
    (toolName) => {
      expect(ARG_REDACTED_TOOLS.has(toolName) || toolName.startsWith("persona_")).toBe(true);
      expect(
        redactToolArgsForLog(toolName, { content: "private persona body" })
      ).toBe("***PERSONA_ARGS_REDACTED***");
    }
  );

  it("leaves non-persona tool arguments unchanged", () => {
    const args = { task_id: "task-1", note: "safe" };

    expect(redactToolArgsForLog("update_task", args)).toBe(args);
  });

  it("redacts persona tool results before logging", () => {
    expect(
      redactToolResultForLog("get_persona_draft", {
        content: "private persona body",
      })
    ).toBe("***PERSONA_ARGS_REDACTED***");
  });
});

describe("redactSecrets — pattern matching", () => {
  // Pattern 1: Anthropic API keys
  it("redacts Anthropic API key (sk-ant-)", () => {
    // "key" contains KEY → generic pattern (11) re-redacts
    const input = "key=sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456";
    expect(redactSecrets(input)).toBe("key=***REDACTED***");
  });

  // Pattern 2: OpenRouter keys
  it("redacts OpenRouter key (sk-or-v1-)", () => {
    const input = "token: sk-or-v1-abcdefghijklmnopqrstuvwxyz1234";
    expect(redactSecrets(input)).toBe("token: sk-or-v1-***REDACTED***");
  });

  // Pattern 3: RalphX API keys
  it("redacts RalphX API key (rxk_live_)", () => {
    // "key" contains KEY → generic pattern (11) re-redacts
    const input = "key=rxk_live_abcdefghijklmnopqrstuvwxyz1234";
    expect(redactSecrets(input)).toBe("key=***REDACTED***");
  });

  // Pattern 4: Generic OpenAI-style keys (catch-all)
  it("redacts generic OpenAI-style key (sk-)", () => {
    // When the env var name also contains KEY/TOKEN, the generic env-var
    // assignment pattern (11) re-redacts after the specific sk- pattern (4)
    const input = "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz1234";
    expect(redactSecrets(input)).toBe("OPENAI_API_KEY=***REDACTED***");
  });

  // Pattern 5: Bearer tokens
  it("redacts Bearer tokens", () => {
    const input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9abc";
    expect(redactSecrets(input)).toBe("Authorization: Bearer ***REDACTED***");
  });

  // Pattern 6: ANTHROPIC_AUTH_TOKEN in JSON
  it("redacts ANTHROPIC_AUTH_TOKEN in JSON", () => {
    const input = '{"ANTHROPIC_AUTH_TOKEN": "sk-ant-secret-key-value-here"}';
    expect(redactSecrets(input)).toBe('{"ANTHROPIC_AUTH_TOKEN":"***REDACTED***"}');
  });

  // Pattern 7: ANTHROPIC_API_KEY in JSON
  it("redacts ANTHROPIC_API_KEY in JSON", () => {
    const input = '{"ANTHROPIC_API_KEY": "sk-ant-api-key-here-longer-value"}';
    expect(redactSecrets(input)).toBe('{"ANTHROPIC_API_KEY":"***REDACTED***"}');
  });

  // Pattern 8: GitHub PATs
  it("redacts GitHub PAT (ghp_)", () => {
    // GITHUB_TOKEN contains TOKEN → generic pattern (11) re-redacts
    const input = "GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz1234";
    expect(redactSecrets(input)).toBe("GITHUB_TOKEN=***REDACTED***");
  });

  // Pattern 9: GitHub OAuth tokens
  it("redacts GitHub OAuth token (gho_)", () => {
    // oauth_token contains token → generic pattern (11) re-redacts
    const input = "oauth_token=gho_abcdefghijklmnopqrstuvwxyz1234";
    expect(redactSecrets(input)).toBe("oauth_token=***REDACTED***");
  });
});

describe("redactSecrets — generic sensitive env var patterns", () => {
  // Pattern 10: JSON format
  it("redacts env var with KEY in name (JSON)", () => {
    const input = '{"OPENAI_API_KEY": "sk-proj-abc123xyz"}';
    const result = redactSecrets(input);
    expect(result).toContain('"OPENAI_API_KEY":"***REDACTED***"');
    expect(result).not.toContain("sk-proj-abc123xyz");
  });

  it("redacts env var with TOKEN in name (JSON)", () => {
    const input = '{"GITHUB_TOKEN": "ghp_somesecretvalue123"}';
    const result = redactSecrets(input);
    expect(result).toContain('"GITHUB_TOKEN":"***REDACTED***"');
    expect(result).not.toContain("ghp_somesecretvalue123");
  });

  it("redacts env var with SECRET in name (JSON)", () => {
    const input = '{"AWS_SECRET_ACCESS_KEY": "wJalrXUtnFEMI/K7MDENG"}';
    const result = redactSecrets(input);
    expect(result).toContain('"AWS_SECRET_ACCESS_KEY":"***REDACTED***"');
    expect(result).not.toContain("wJalrXUtnFEMI");
  });

  it("redacts env var with PASSWORD in name (JSON)", () => {
    const input = '{"DB_PASSWORD": "hunter2"}';
    const result = redactSecrets(input);
    expect(result).toContain('"DB_PASSWORD":"***REDACTED***"');
    expect(result).not.toContain("hunter2");
  });

  it("redacts env var with CREDENTIAL in name (JSON)", () => {
    const input = '{"SERVICE_CREDENTIAL": "abc-secret-456"}';
    const result = redactSecrets(input);
    expect(result).toContain('"SERVICE_CREDENTIAL":"***REDACTED***"');
    expect(result).not.toContain("abc-secret-456");
  });

  it("is case-insensitive for env var key names (JSON)", () => {
    const input = '{"openai_api_key": "myvalue123"}';
    const result = redactSecrets(input);
    expect(result).toContain('"openai_api_key":"***REDACTED***"');
    expect(result).not.toContain("myvalue123");
  });

  it("does not redact non-sensitive JSON keys", () => {
    const input = '{"TAURI_API_URL": "http://127.0.0.1:3847"}';
    expect(redactSecrets(input)).toBe(input);
  });

  // Pattern 11: Assignment format
  it("redacts env var with KEY in name (assignment)", () => {
    const input = "OPENAI_API_KEY=sk-proj-abc123xyz456";
    expect(redactSecrets(input)).toBe("OPENAI_API_KEY=***REDACTED***");
  });

  it("redacts env var with TOKEN in name (assignment)", () => {
    const input = "GITHUB_TOKEN=ghp_somesecretvalue123456";
    expect(redactSecrets(input)).toBe("GITHUB_TOKEN=***REDACTED***");
  });

  it("redacts env var with SECRET in name (assignment)", () => {
    const input = "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI";
    expect(redactSecrets(input)).toBe("AWS_SECRET_ACCESS_KEY=***REDACTED***");
  });

  it("redacts quoted assignment values", () => {
    const input = 'MY_SECRET_KEY="some secret value"';
    expect(redactSecrets(input)).toBe("MY_SECRET_KEY=***REDACTED***");
  });

  it("does not redact non-sensitive assignment keys", () => {
    const input = "TAURI_API_URL=http://127.0.0.1:3847";
    expect(redactSecrets(input)).toBe(input);
  });

  it("redacts multiple sensitive env vars in a line", () => {
    const input = "OPENAI_API_KEY=sk-abc123 GITHUB_TOKEN=ghp_def456 HOME=/Users/me";
    const result = redactSecrets(input);
    expect(result).toContain("OPENAI_API_KEY=***REDACTED***");
    expect(result).toContain("GITHUB_TOKEN=***REDACTED***");
    expect(result).toContain("HOME=/Users/me");
  });
});

describe("redactSecrets — non-secrets pass through", () => {
  it("preserves plain log messages", () => {
    const input = "[RalphX MCP] Starting server...";
    expect(redactSecrets(input)).toBe(input);
  });

  it("preserves short sk- prefixes that are not secrets", () => {
    // Less than 20 chars after sk-
    const input = "sk-short123";
    expect(redactSecrets(input)).toBe(input);
  });

  it("preserves non-secret environment variable names", () => {
    const input = "TAURI_API_URL=http://127.0.0.1:3847";
    expect(redactSecrets(input)).toBe(input);
  });

  it("preserves empty string", () => {
    expect(redactSecrets("")).toBe("");
  });

  it("preserves short Bearer values", () => {
    // Less than 20 chars after Bearer
    const input = "Bearer shorttoken";
    expect(redactSecrets(input)).toBe(input);
  });
});

describe("redactSecrets — ordering (specific before generic)", () => {
  it("redacts sk-ant- before the generic sk- catch-all (no double-redaction)", () => {
    // When the assignment key also contains KEY/TOKEN, generic pattern re-redacts
    const input = "key=sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456";
    const result = redactSecrets(input);
    expect(result).toBe("key=***REDACTED***");
    expect(result).not.toContain("sk-ant-api03");
  });

  it("redacts sk-or-v1- before the generic sk- catch-all", () => {
    // "token" contains TOKEN → generic pattern re-redacts
    const input = "token=sk-or-v1-abcdefghijklmnopqrstuvwxyz1234";
    const result = redactSecrets(input);
    expect(result).toBe("token=***REDACTED***");
    expect(result).not.toContain("sk-or-v1-abcdefghijklmnopqrstuvwxyz");
  });
});

describe("redactSecrets — multi-secret lines", () => {
  it("redacts multiple secrets on the same line", () => {
    // key1/key2 contain "key" → generic pattern (11) re-redacts after specific patterns
    const input = "key1=sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456 key2=ghp_abcdefghijklmnopqrstuvwxyz1234";
    const result = redactSecrets(input);
    expect(result).toBe("key1=***REDACTED*** key2=***REDACTED***");
  });

  it("redacts secrets in JSON settings string", () => {
    const input = '{"ANTHROPIC_AUTH_TOKEN": "sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456", "OTHER": "value"}';
    const result = redactSecrets(input);
    expect(result).toContain('"ANTHROPIC_AUTH_TOKEN":"***REDACTED***"');
    expect(result).not.toContain("sk-ant-api03");
  });
});

describe("redactSecrets — edge cases", () => {
  it("handles partial pattern matches without redacting", () => {
    // 'sk-' with exactly 19 chars (one short of 20 minimum)
    const input = "sk-1234567890123456789"; // 19 chars after sk-
    expect(redactSecrets(input)).toBe(input);
  });

  it("handles rxk_live_ with exactly 20 char suffix", () => {
    const input = "rxk_live_12345678901234567890"; // exactly 20 chars
    expect(redactSecrets(input)).toBe("rxk_live_***REDACTED***");
  });
});

describe("safeError — integration", () => {
  it("is callable without throwing", () => {
    expect(() => safeError("[RalphX MCP] test message", { key: "value" })).not.toThrow();
  });

  it("accepts Error objects", () => {
    const err = new Error("sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456");
    expect(() => safeError("Error:", err)).not.toThrow();
  });
});

describe("safeTrace — file logging", () => {
  it("uses module-owned trace dir instead of creating target-project .artifacts", async () => {
    const originalCwd = process.cwd();
    const targetProject = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-target-project-"));
    process.env.RALPHX_WORKING_DIRECTORY = targetProject;

    try {
      process.chdir(targetProject);
      vi.resetModules();
      const isolated = await import("../redact.js");

      isolated.safeTrace("tool.request", {
        api_key: "sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456",
      });

      const logPath = isolated.getTraceLogPath();
      expect(logPath.startsWith(targetProject)).toBe(false);
      expect(logPath).toContain(`${path.sep}.artifacts${path.sep}logs${path.sep}mcp-proxy${path.sep}`);
      expect(fs.existsSync(path.join(targetProject, ".artifacts"))).toBe(false);
    } finally {
      process.chdir(originalCwd);
    }
  });

  it("uses configured app-owned trace dir when it is outside the target project", () => {
    const traceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-mcp-trace-root-"));
    const traceDir = path.join(traceRoot, "logs", "mcp-proxy");
    const targetProject = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-target-project-"));
    fs.mkdirSync(traceDir, { recursive: true });
    process.env.RALPHX_MCP_TRACE_DIR = traceDir;
    process.env.RALPHX_WORKING_DIRECTORY = targetProject;

    const logPath = getTraceLogPath();

    expect(logPath.startsWith(fs.realpathSync.native(traceDir) + path.sep)).toBe(true);
    expect(fs.existsSync(traceDir)).toBe(true);
    expect(fs.existsSync(path.join(targetProject, ".artifacts"))).toBe(false);
  });

  it("continues without throwing when configured trace dir cannot be created", () => {
    const blockedRoot = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-blocked-trace-root-"));
    const traceDir = path.join(blockedRoot, "logs", "mcp-proxy");
    process.env.RALPHX_MCP_TRACE_DIR = traceDir;
    fs.chmodSync(blockedRoot, 0o555);

    try {
      expect(() => safeTrace("server.start")).not.toThrow();
      const logPath = getTraceLogPath();
      expect(logPath.startsWith(traceDir + path.sep)).toBe(false);
    } finally {
      fs.chmodSync(blockedRoot, 0o755);
    }
  });

  it("writes only minimal allowlisted trace metadata under the safe trace root", () => {
    process.env.RALPHX_AGENT_TYPE = "ralphx-ideation";
    process.env.RALPHX_CONTEXT_TYPE = "ideation";
    process.env.RALPHX_CONTEXT_ID = "session-123";

    safeTrace("tool.request", {
      api_key: "sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456",
    });

    const logPath = getTraceLogPath();
    const contents = fs.readFileSync(logPath, "utf8");
    expect(logPath).toContain(`${path.sep}.artifacts${path.sep}logs${path.sep}mcp-proxy${path.sep}`);
    expect(contents).toContain("\"event\":\"tool.request\"");
    expect(contents).not.toContain("abcdefghijklmnopqrstuvwxyz123456");
    expect(contents).not.toContain("sk-ant-***REDACTED***");
    expect(contents).not.toContain("ralphx-ideation");
    expect(contents).not.toContain("session-123");
  });

  it("rejects trace dir overrides inside the target working directory", () => {
    const originalCwd = process.cwd();
    const targetProject = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-target-project-"));
    const unsafeTraceDir = path.join(targetProject, ".artifacts/logs/mcp-proxy");
    process.env.RALPHX_MCP_TRACE_DIR = unsafeTraceDir;
    process.env.RALPHX_WORKING_DIRECTORY = targetProject;

    try {
      process.chdir(targetProject);

      safeTrace("tool.request", {
        api_key: "sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456",
      });

      const logPath = getTraceLogPath();
      expect(logPath.startsWith(targetProject)).toBe(false);
      expect(logPath.startsWith(unsafeTraceDir)).toBe(false);
      expect(fs.existsSync(path.join(targetProject, ".artifacts"))).toBe(false);
    } finally {
      process.chdir(originalCwd);
    }
  });

  it("rejects trace dir overrides without the fixed mcp-proxy leaf", () => {
    const traceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-mcp-trace-root-"));
    const unsafeTraceDir = path.join(traceRoot, "logs", "custom-leaf");
    process.env.RALPHX_MCP_TRACE_DIR = unsafeTraceDir;

    safeTrace("tool.request");

    const logPath = getTraceLogPath();
    expect(logPath.startsWith(unsafeTraceDir + path.sep)).toBe(false);
    expect(fs.existsSync(unsafeTraceDir)).toBe(false);
  });

  it("rejects relative trace dir overrides", () => {
    const unsafeTraceDir = path.join("relative", "logs", "mcp-proxy");
    process.env.RALPHX_MCP_TRACE_DIR = unsafeTraceDir;

    safeTrace("tool.request");

    const logPath = getTraceLogPath();
    expect(logPath.startsWith(unsafeTraceDir + path.sep)).toBe(false);
    expect(fs.existsSync(unsafeTraceDir)).toBe(false);
  });

  it("rejects trace dir overrides symlinked into the target working directory", () => {
    const traceRoot = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-mcp-trace-root-"));
    const targetProject = fs.mkdtempSync(path.join(os.tmpdir(), "ralphx-target-project-"));
    const logsDir = path.join(traceRoot, "logs");
    const unsafeTraceDir = path.join(logsDir, "mcp-proxy");
    fs.mkdirSync(logsDir, { recursive: true });
    fs.symlinkSync(targetProject, unsafeTraceDir, "dir");
    process.env.RALPHX_MCP_TRACE_DIR = unsafeTraceDir;
    process.env.RALPHX_WORKING_DIRECTORY = targetProject;

    safeTrace("tool.request");

    const logPath = getTraceLogPath();
    expect(logPath.startsWith(targetProject + path.sep)).toBe(false);
    expect(logPath.startsWith(unsafeTraceDir + path.sep)).toBe(false);
    expect(fs.existsSync(path.join(targetProject, ".artifacts"))).toBe(false);
  });

  it("normalizes non-allowlisted event names", () => {
    safeTrace("tool.request:user-supplied");

    const contents = fs.readFileSync(getTraceLogPath(), "utf8");
    expect(contents).toContain("\"event\":\"unknown\"");
    expect(contents).not.toContain("tool.request:user-supplied");
  });
});
