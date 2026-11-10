// Generated from Rust serde types. Run npm run generate:contracts.

export type EnvironmentId = string;
export type ThreadId = string;
export type ThreadRef = { environmentId: EnvironmentId, threadId: ThreadId, };
export type EnvironmentSelection = "all" | { "one": EnvironmentId };
export type EnvironmentMachineKind = "server" | "cloud" | "linux" | "desktop" | "laptop" | "mac-mini" | "mac-studio";
export type EnvironmentAccess = "direct" | "cloud" | "directOrCloud" | "unsupported";
export type EnvironmentView = { id: EnvironmentId, label: string, machine: EnvironmentMachineKind, status: string, access: EnvironmentAccess,
/**
 * Whether Bar holds, or could hold, its own direct or SSH grant that the
 * user may remove. False for the local environment and Relay-only sources.
 */
canForgetAccess: boolean, };
