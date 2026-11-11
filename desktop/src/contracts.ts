export type Screen = 'threads' | 'usage' | 'settings';
export type Metric = 'cost' | 'tokens' | 'limits';
export type Period = '24h' | '7d' | '30d' | '90d';
export type ThreadStatus = 'Working' | 'Goal' | 'Approval' | 'Input' | 'Request' | 'Woke' | 'Failed' | 'Waiting' | 'Limited' | 'Done' | 'Settled';

export interface ThreadItem {
  id: string;
  projectId: string;
  projectTitle: string;
  title: string;
  status: ThreadStatus;
  isPinned: boolean;
  providers: string[];
  branch: string | null;
  modelName: string | null;
  prNumber: number | null;
  updatedAt: string;
  latestUserMessageAt: string | null;
  latestRunCompletedAt: string | null;
  activeRunStartedAt: string | null;
  usageLimitResetAt: string | null;
  projectIcon: string | null;
  projectFavicon?: string | null;
}
export interface Preview { text: string; updatedAt: string | null }
export interface Tokens { uncachedInput: number; cachedInput: number; cacheCreation: number; output: number; reasoning: number }
export interface ProviderUsage { kind: string; label: string; costUsd: number; processedTokens: number; sessions: number; records: number }
export interface ModelUsage { kind: string; name: string; costUsd: number; processedTokens: number; records: number; unpricedRecords: number }
export interface ChartSlot { label: string; day: number; hourStartMs: number | null; costUsd: number; processedTokens: number; providers: { kind: string; costUsd: number; processedTokens: number }[] }
export interface UsageReport {
  costUsd: number;
  processedTokens: number;
  tokens: Tokens;
  sessions: number;
  records: number;
  unpricedRecords: number;
  duplicatesDropped: number;
  cacheSavingsUsd: number;
  malformedRecords: number;
  rejectedFiles: number;
  cacheModified: string | null;
  scanCacheMissing: boolean;
  localSourcesPartial: boolean;
  costByType: Record<string, number>;
  costBySpeed: Record<string, number>;
  providers: ProviderUsage[];
  models: ModelUsage[];
  timeline: ChartSlot[];
  resolution: 'hour' | 'day';
  timeZone: string;
  untilMs: number;
  notices: string[];
}
export interface LimitMember { label: string; remainingPercent: number; resetAt: string | null; plan: string | null; evenPaceLeft?: number | null; refillPercent?: number; resetCredits?: number | null }
export interface LimitRow { id: string; label: string; remainingPercent: number; usedPercent: number; resetAt: string | null; pace: string | null; members: LimitMember[] }
export interface LimitGroup { driver: string; label: string; rows: LimitRow[] }
export interface LimitsReport { groups: LimitGroup[]; notices: string[]; checkedAt: string | null; readFailed?: boolean; skippedFiles?: number; links: { label: string; url: string; message?: string | null; accounts?: string[] }[] }
export interface ConnectionState { status: string; message: string; connected: boolean; paired: boolean; hasData: boolean; port: number; updatedAt: string | null }
export interface Settings { notificationsMuted: boolean; showsPreviews: boolean; automaticallyChecksForUpdates: boolean; installsWithoutAsking: boolean }
export interface UpdateState { status: string; message: string; version: string | null; offerId: number | null; canInstall: boolean; busy: boolean }
export interface AppSnapshot {
  startupError?: string | null;
  now: string;
  threads: ThreadItem[];
  online: boolean;
  threadError: string | null;
  loading: boolean;
  connection: ConnectionState;
  usage: UsageReport | null;
  usageError: string | null;
  usageLoading: boolean;
  usagePeriod: Period;
  limits: LimitsReport | null;
  settings: Settings;
  update: UpdateState;
  version: string;
}
