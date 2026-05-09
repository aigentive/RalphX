/**
 * Static guide content for v1_get_agent_guide.
 * All content is pure markdown — no backend dependency, no state dependency.
 *
 * IMPORTANT: ALL_TOOL_NAMES must stay in sync with TOOL_CATEGORIES in index.ts.
 * The bidirectional sync test validates: TOOL_CATEGORIES ↔ ALL_TOOL_NAMES ↔ FULL_GUIDE content.
 */
export type GuideSection = "setup" | "overview" | "discovery" | "ideation" | "tasks" | "pipeline" | "events" | "patterns" | "resilience";
export declare const GUIDE_SECTIONS: Record<GuideSection, string>;
export declare const VALID_SECTIONS: GuideSection[];
export declare const FULL_GUIDE: string;
/**
 * Canonical list of all MCP tools.
 * Used by tests to verify guide completeness (bidirectional sync with TOOL_CATEGORIES in index.ts).
 *
 * When adding new tools: update TOOL_CATEGORIES in index.ts AND add here AND document in GUIDE_SECTIONS.
 */
export declare const ALL_TOOL_NAMES: string[];
//# sourceMappingURL=guide-content.d.ts.map