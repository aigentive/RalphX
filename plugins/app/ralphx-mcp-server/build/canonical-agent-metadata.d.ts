type CanonicalAgentDefinition = {
    name: string;
    delegation?: {
        allowed_targets?: string[];
    };
    capabilities?: {
        mcp_tools?: string[];
    };
    profiles?: Record<string, {
        delegation?: {
            allowed_targets?: string[];
        };
        capabilities?: {
            mcp_tools?: string[];
        };
    }>;
};
export declare const SAFE_CANONICAL_PROFILE_NAME: RegExp;
export declare function resolveRepoRoot(): string;
export declare function canonicalAgentName(agentType: string): string;
export declare function clearCanonicalAgentDefinitionCache(): void;
export declare function loadCanonicalAgentDefinition(agentType: string): CanonicalAgentDefinition | null;
export declare function loadCanonicalAgentDefinitionForProfile(agentType: string, agentProfile?: string): CanonicalAgentDefinition | null;
export declare function loadCanonicalMcpTools(agentType: string, agentProfile?: string): string[] | undefined;
export {};
//# sourceMappingURL=canonical-agent-metadata.d.ts.map