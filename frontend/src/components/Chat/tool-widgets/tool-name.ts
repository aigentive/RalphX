const MCP_RALPHX_PREFIX = "mcp__ralphx__";
const MCP_RALPHX_INTERNAL_PREFIX = "mcp__ralphx_internal__";
const RALPHX_SERVER_PREFIX = "ralphx:";
const RALPHX_INTERNAL_SERVER_PREFIX = "ralphx_internal:";
const RALPHX_DOUBLE_COLON_PREFIX = "ralphx::";
const RALPHX_INTERNAL_DOUBLE_COLON_PREFIX = "ralphx_internal::";
const TOOL_ALIASES: Record<string, string> = {
  fs_read_file: "read",
  fs_grep: "grep",
  fs_glob: "glob",
};

function applyToolAlias(toolName: string): string {
  return TOOL_ALIASES[toolName] ?? toolName;
}

export function canonicalizeToolName(toolName: string): string {
  const normalized = toolName.trim().toLowerCase();

  if (normalized.startsWith(MCP_RALPHX_PREFIX)) {
    return applyToolAlias(normalized.slice(MCP_RALPHX_PREFIX.length));
  }

  if (normalized.startsWith(MCP_RALPHX_INTERNAL_PREFIX)) {
    return applyToolAlias(normalized.slice(MCP_RALPHX_INTERNAL_PREFIX.length));
  }

  if (normalized.startsWith(RALPHX_DOUBLE_COLON_PREFIX)) {
    return applyToolAlias(normalized.slice(RALPHX_DOUBLE_COLON_PREFIX.length));
  }

  if (normalized.startsWith(RALPHX_INTERNAL_DOUBLE_COLON_PREFIX)) {
    return applyToolAlias(normalized.slice(RALPHX_INTERNAL_DOUBLE_COLON_PREFIX.length));
  }

  if (normalized.startsWith(RALPHX_SERVER_PREFIX)) {
    return applyToolAlias(normalized.slice(RALPHX_SERVER_PREFIX.length));
  }

  if (normalized.startsWith(RALPHX_INTERNAL_SERVER_PREFIX)) {
    return applyToolAlias(normalized.slice(RALPHX_INTERNAL_SERVER_PREFIX.length));
  }

  return applyToolAlias(normalized);
}

export function getToolCallLookupCandidates(toolName: string): string[] {
  const normalized = toolName.trim().toLowerCase();
  const canonical = canonicalizeToolName(normalized);
  const candidates = [normalized];

  if (!candidates.includes(canonical)) {
    candidates.push(canonical);
  }

  const mcpCandidate = `${MCP_RALPHX_PREFIX}${canonical}`;
  if (!candidates.includes(mcpCandidate)) {
    candidates.push(mcpCandidate);
  }

  const internalMcpCandidate = `${MCP_RALPHX_INTERNAL_PREFIX}${canonical}`;
  if (!candidates.includes(internalMcpCandidate)) {
    candidates.push(internalMcpCandidate);
  }

  const serverCandidate = `${RALPHX_SERVER_PREFIX}${canonical}`;
  if (!candidates.includes(serverCandidate)) {
    candidates.push(serverCandidate);
  }

  const internalServerCandidate = `${RALPHX_INTERNAL_SERVER_PREFIX}${canonical}`;
  if (!candidates.includes(internalServerCandidate)) {
    candidates.push(internalServerCandidate);
  }

  const doubleColonCandidate = `${RALPHX_DOUBLE_COLON_PREFIX}${canonical}`;
  if (!candidates.includes(doubleColonCandidate)) {
    candidates.push(doubleColonCandidate);
  }

  const internalDoubleColonCandidate = `${RALPHX_INTERNAL_DOUBLE_COLON_PREFIX}${canonical}`;
  if (!candidates.includes(internalDoubleColonCandidate)) {
    candidates.push(internalDoubleColonCandidate);
  }

  return candidates;
}
