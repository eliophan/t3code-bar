//! Read-only T3 Code usage data. This crate does not depend on Tauri.

mod cache;
mod selection;

pub use cache::{decode_cache, CacheError, Dataset, File, Record, Source, Speed};
pub use selection::{Period, Selection, Slot, Tally, Window};
mod pricing;
pub use pricing::{Categories, ModelRate, PriceBook, PricedRecord, TokenRates};
mod report;
pub use report::{
    Amount, Column, CostBySpeed, CostByType, Model, Provider, Timeline, Tokens, UsageReport,
};
mod reader;
pub use reader::{UsageError, UsageReader};
mod tier;
pub use tier::resolve_codex_tiers;
mod local;
pub use local::{LocalRoots, LocalStatus};
mod antigravity;
mod merge;
pub use merge::{
    merge_usage, EnvironmentUsage, MergedUsage, UsageBucket, UsageCategoryCost,
    UsageContractMismatch, UsageContribution, UsageCostQuality, UsageModelFilter,
    UsageModelSelector, UsagePricing, UsageQuery, UsageSource, UsageSourceFingerprint,
    UsageSourceStatus, UsageSummary, UsageTokenTotals, USAGE_COLLATION_LOCALE,
    USAGE_CONTRACT_VERSION, USAGE_MERGE_COMPATIBLE_SINCE,
};

mod contribution;
pub use contribution::{LocalContribution, LocalProvenance};
