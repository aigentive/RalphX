export declare function expandHome(value: string): string;
export declare function normalizePathLike(value: string): string;
export declare function isWithin(root: string, candidate: string): boolean;
export declare function getPrimaryFilesystemRoot(): string;
export declare function getConfiguredFilesystemReadRoots(): string[];
export declare function getAllowedFilesystemRoots(): string[];
export declare function resolveEnforcedFilesystemPath(inputPath: string, displayPath: string): Promise<string>;
export declare function resolveScopedFilesystemPath(inputPath: string, basePath?: string): string;
//# sourceMappingURL=path-policy.d.ts.map