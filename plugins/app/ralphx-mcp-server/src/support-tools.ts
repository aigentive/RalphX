/**
 * Support and infrastructure MCP tool definitions
 */

import { Tool } from "@modelcontextprotocol/sdk/types.js";

export const SUPPORT_TOOLS: Tool[] = [
  // ========================================================================
  // MEMORY WRITE TOOLS (memory agents only - restricted via allowlist)
  // ========================================================================
  {
    name: "upsert_memories",
    description:
      "Batch upsert memory entries to SQLite canonical storage. " +
      "Performs content-hash deduplication to prevent duplicates. " +
      "WRITE-ONLY tool restricted to ralphx-memory-maintainer and ralphx-memory-capture agents.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID (from RALPHX_PROJECT_ID env var)",
        },
        memories: {
          type: "array",
          items: {
            type: "object",
            properties: {
              bucket: {
                type: "string",
                enum: ["architecture_patterns", "implementation_discoveries", "operational_playbooks"],
                description: "Memory bucket classification",
              },
              title: {
                type: "string",
                description: "Concise title for this memory (50-80 chars)",
              },
              summary: {
                type: "string",
                description: "Brief summary suitable for rule index files (1-3 sentences)",
              },
              details_markdown: {
                type: "string",
                description: "Full markdown details with examples, context, and rationale",
              },
              scope_paths: {
                type: "array",
                items: { type: "string" },
                description: "Glob patterns for path scoping (e.g., ['src/domain/**', 'src-tauri/src/application/**'])",
              },
              source_context_type: {
                type: "string",
                description: "Optional: context type (e.g., 'task_execution', 'planning', 'review')",
              },
              source_context_id: {
                type: "string",
                description: "Optional: source context ID (e.g., task_id, session_id)",
              },
              source_conversation_id: {
                type: "string",
                description: "Optional: conversation ID for traceability",
              },
              quality_score: {
                type: "number",
                description: "Optional: quality score 0-1 (higher = more valuable)",
              },
            },
            required: ["bucket", "title", "summary", "details_markdown", "scope_paths"],
          },
          description: "Array of memory entries to upsert",
        },
      },
      required: ["project_id", "memories"],
    },
  },
  {
    name: "mark_memory_obsolete",
    description:
      "Mark a memory entry as obsolete (soft delete). " +
      "The memory remains in DB but is excluded from index generation and searches. " +
      "WRITE-ONLY tool restricted to ralphx-memory-maintainer agent.",
    inputSchema: {
      type: "object",
      properties: {
        memory_id: {
          type: "string",
          description: "The memory entry ID to mark obsolete",
        },
      },
      required: ["memory_id"],
    },
  },
  {
    name: "refresh_memory_rule_index",
    description:
      "Regenerate .claude/rules/ index files from DB canonical state. " +
      "Reads memory entries for project, groups by scope_key, and writes index files with summaries + memory IDs. " +
      "WRITE-ONLY tool restricted to ralphx-memory-maintainer agent.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID",
        },
        scope_key: {
          type: "string",
          description: "Optional: specific scope_key to refresh. If omitted, refreshes all rule indexes for project.",
        },
      },
      required: ["project_id"],
    },
  },
  {
    name: "ingest_rule_file",
    description:
      "Ingest a .claude/rules/*.md file into canonical memory DB. " +
      "Parses content into chunks, classifies buckets, upserts to memory_entries, " +
      "rewrites file to index format, and enqueues archive jobs. " +
      "WRITE-ONLY tool restricted to ralphx-memory-maintainer agent.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID",
        },
        rule_file_path: {
          type: "string",
          description: "Path to rule file relative to project root (e.g., '.claude/rules/task-state-machine.md')",
        },
      },
      required: ["project_id", "rule_file_path"],
    },
  },
  {
    name: "rebuild_archive_snapshots",
    description:
      "Enqueue full rebuild of archive snapshots from DB canonical state. " +
      "Generates RalphX-owned app-data archive snapshots for disaster recovery. " +
      "WRITE-ONLY tool restricted to ralphx-memory-maintainer agent.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID",
        },
      },
      required: ["project_id"],
    },
  },
  {
    name: "get_conversation_transcript",
    description:
      "Retrieve conversation messages for a given conversation ID, ordered chronologically. Used by ralphx-memory-capture for analysis.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description: "The conversation ID",
        },
      },
      required: ["conversation_id"],
    },
  },
  {
    name: "propose_plan_mode",
    description:
      "Ask the user whether the current project Chat/Edit conversation should switch to Plan mode before continuing. " +
      "Use when the request is broad, planning-heavy, or needs user-owned decisions before implementation. " +
      "This blocks until the user accepts, declines, or skips. If accepted, the UI switches the active conversation to Plan mode; do not start ideation yourself in the same turn.",
    inputSchema: {
      type: "object",
      properties: {
        conversation_id: {
          type: "string",
          description:
            "Optional active agent conversation ID. Omit when RalphX provides the current conversation id in the MCP runtime context.",
        },
        current_mode: {
          type: "string",
          enum: ["chat", "edit"],
          description: "The current conversation mode when known.",
        },
        reason: {
          type: "string",
          description:
            "Concise user-facing reason Plan mode would help, such as needing requirements or a structured plan.",
        },
        question: {
          type: "string",
          description:
            "Optional exact question text. If omitted, RalphX generates one from reason.",
        },
      },
    },
  },

  // ========================================================================
  // PROJECT ANALYSIS TOOLS (worker/reviewer/merger + ralphx-project-analyzer agents)
  // ========================================================================
  {
    name: "get_project_analysis",
    description:
      "Get project analysis data including build commands, validation commands, and worktree setup instructions. " +
      "Returns path-scoped entries with resolved template variables ({project_root}, {worktree_path}, {task_branch}). " +
      "If analysis hasn't been run yet, returns { status: 'analyzing', retry_after_secs: 30 }.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID (from RALPHX_PROJECT_ID env var)",
        },
        task_id: {
          type: "string",
          description: "Optional task ID for resolving {worktree_path} and {task_branch} template variables",
        },
      },
      required: ["project_id"],
    },
  },
  {
    name: "save_project_analysis",
    description:
      "Save auto-detected project analysis data. Updates detected_analysis and analyzed_at fields. " +
      "Never touches custom_analysis (user overrides). Only callable by the ralphx-project-analyzer agent.",
    inputSchema: {
      type: "object",
      properties: {
        project_id: {
          type: "string",
          description: "The project ID",
        },
        entries: {
          type: "array",
          items: {
            type: "object",
            properties: {
              path: {
                type: "string",
                description: "Subpath relative to project root (e.g., '.', 'src-tauri/')",
              },
              label: {
                type: "string",
                description: "Human-readable label (e.g., 'Node.js root', 'Rust backend')",
              },
              install: {
                type: "string",
                description: "Install command (e.g., 'npm install'). Null if not needed.",
              },
              validate: {
                type: "array",
                items: { type: "string" },
                description:
                  "Auto-detected analysis must leave this empty. Task agents select focused commands after they know the changed surface; user-managed custom analysis remains separate.",
              },
              worktree_setup: {
                type: "array",
                items: { type: "string" },
                description: "Commands to run in worktree setup (e.g., ['ln -s {project_root}/node_modules {worktree_path}/node_modules'])",
              },
            },
            required: ["path", "label"],
          },
          description: "Array of path-scoped analysis entries",
        },
      },
      required: ["project_id", "entries"],
    },
  },
  // ========================================================================
  // CROSS-PROJECT TOOLS (ralphx-ideation)
  // ========================================================================
  {
    name: "list_projects",
    description:
      "List all RalphX projects on this instance. Returns project id, name, working_directory, and status for each project. " +
      "Use to discover existing projects before creating cross-project sessions.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
  {
    name: "create_cross_project_session",
    description:
      "Create a new ideation session in a target project with an inherited plan from the current project. " +
      "The backend resolves the target project by filesystem path (auto-creates a RalphX project if none exists at that path). " +
      "The inherited plan is set to 'imported_verified' status — no auto-verify triggered. " +
      "Use when exporting a verified plan to another project for execution.",
    inputSchema: {
      type: "object",
      properties: {
        target_project_path: {
          type: "string",
          description:
            "Absolute filesystem path to the target project root. Backend resolves or auto-creates the RalphX project.",
        },
        source_session_id: {
          type: "string",
          description:
            "ID of the source ideation session whose verified plan will be inherited.",
        },
        title: {
          type: "string",
          description:
            "Optional title for the new session. Defaults to 'Imported: {source session title}'.",
        },
      },
      required: ["target_project_path", "source_session_id"],
    },
  },
  {
    name: "cross_project_guide",
    description:
      "Analyze a plan for cross-project paths and return structured guidance for multi-project orchestration. " +
      "Detects file paths referencing different project roots, suggests how to split proposals across projects, " +
      "and provides step-by-step instructions for creating sessions in target projects. " +
      "When session_id is provided: calls the backend to set the cross-project gate, unlocking proposal creation (gate_status: 'set'). " +
      "Without session_id: returns analysis only, gate is not set (gate_status: 'no_session_id').",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description:
            "Session ID to fetch the plan content from (uses get_session_plan internally). " +
            "Provide either session_id or plan_content, not both.",
        },
        plan_content: {
          type: "string",
          description:
            "Raw plan text to analyze directly. " +
            "Provide either plan_content or session_id, not both.",
        },
      },
      required: [],
    },
  },
  {
    name: "migrate_proposals",
    description:
      "Copy proposals from a source ideation session to a target ideation session. " +
      "Each proposal is cloned with a new UUID; migrated_from traceability fields are set automatically. " +
      "Dependencies between migrated proposals are remapped to the new IDs. " +
      "Dependencies to proposals outside the migration set are dropped with warnings in the response. " +
      "Use this to move cross-project proposals to the correct project session after using create_cross_project_session.",
    inputSchema: {
      type: "object",
      properties: {
        source_session_id: {
          type: "string",
          description: "ID of the source ideation session to copy proposals from.",
        },
        target_session_id: {
          type: "string",
          description: "ID of the target ideation session to copy proposals into.",
        },
        proposal_ids: {
          type: "array",
          items: { type: "string" },
          description:
            "Optional list of proposal IDs to migrate. If omitted, all proposals from the source session are considered (subject to target_project_filter).",
        },
        target_project_filter: {
          type: "string",
          description:
            "Optional: only migrate proposals whose target_project field matches this string. " +
            "Useful for migrating only the proposals intended for a specific project.",
        },
      },
      required: ["source_session_id", "target_session_id"],
    },
  },
  // ========================================================================
  // CHILD SESSION TOOLS (ralphx-ideation)
  // ========================================================================
  {
    name: "get_child_session_status",
    description:
      "Returns live status of a child session: session metadata, agent process state (idle/likely_generating/likely_waiting), " +
      "and recent messages. Use to monitor child session progress or verify agent completion. " +
      "Set include_recent_messages=true so you can inspect the last assistant/tool outputs instead of guessing what happened.",
    inputSchema: {
      type: "object",
      examples: [
        {
          session_id: "child-session-id",
          include_recent_messages: true,
          message_limit: 10,
        },
      ],
      properties: {
        session_id: {
          type: "string",
          description: "The child session ID to check",
        },
        include_recent_messages: {
          type: "boolean",
          description: "Include recent conversation messages (default: false)",
        },
        message_limit: {
          type: "number",
          description:
            "Max messages to return (default: 5, max: 50). Only used when include_recent_messages=true.",
        },
      },
      required: ["session_id"],
    },
  },
  {
    name: "send_ideation_session_message",
    description:
      "Sends a message to any ideation session's agent conversation by session ID. " +
      "Zero directionality enforcement — any session can message any active session (parent→child, child→parent, or peer). " +
      "If agent is generating, message is queued. If agent is idle, a new agent run is spawned. " +
      "Returns delivery_status: 'sent' (written to active stdin), " +
      "'queued' (agent busy, will receive on next turn), or 'spawned' (new agent run started). " +
      "Use to inject context, send escalation payloads, or send stop signals. " +
      "Repeat the full task context and expected outcome instead of sending a vague follow-up.",
    inputSchema: {
      type: "object",
      examples: [
        {
          session_id: "child-session-id",
          message:
            "Please finish the recovery-path review and publish a concise team artifact with evidence for any remaining blocker.",
        },
      ],
      properties: {
        session_id: {
          type: "string",
          description: "The target ideation session ID to message",
        },
        message: {
          type: "string",
          description: "The message content to send to the session's agent",
        },
      },
      required: ["session_id", "message"],
    },
  },
];
