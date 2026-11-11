//! Read-only T3 Limits cache models and layout.
use bar_identity::{EnvironmentId, EnvironmentSelection};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: String,
    pub kind: WindowKind,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub duration_mins: Option<f64>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum WindowKind {
    Session,
    Weekly,
    Monthly,
    Other,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredits {
    pub available_count: i64,
    pub next_expires_at: Option<DateTime<Utc>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub driver: String,
    pub display_name: Option<String>,
    pub plan: Option<String>,
    pub checked_at: DateTime<Utc>,
    pub windows: Vec<Window>,
    pub reset_credits: Option<ResetCredits>,
    pub name_order: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub label: String,
    pub url: String,
    pub message: Option<String>,
    pub accounts: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsSnapshot {
    pub accounts: Vec<Account>,
    pub notices: Vec<String>,
    pub links: Vec<Link>,
    pub state: LimitsState,
    pub skipped_files: usize,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LimitsState {
    NoCacheDirectory,
    NoLimitsReported,
    Ok,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub account: Account,
    pub window: Window,
    pub refill_percent: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub account: Account,
    pub member: Option<Member>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reset {
    pub account_id: String,
    pub at: DateTime<Utc>,
    pub restores_percent: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub kind: WindowKind,
    pub label: String,
    pub members: Vec<Member>,
    pub columns: Vec<Column>,
    pub used_percent: i64,
    pub remaining_percent: i64,
    pub pace: Option<Pace>,
    pub resets: Vec<Reset>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Pace {
    Ahead,
    On,
    Under,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub driver: String,
    pub accounts: Vec<Account>,
    pub rows: Vec<Row>,
}

fn date(v: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v?.as_str()?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}
fn string(v: Option<&Value>) -> Option<String> {
    v?.as_str().map(str::to_owned)
}
fn rounded(n: f64) -> i64 {
    (n + 0.5).floor() as i64
}
impl Window {
    pub fn left_percent(&self) -> i64 {
        rounded(100.0 - self.used_percent.clamp(0.0, 100.0))
    }
    pub fn elapsed_fraction(&self, now: DateTime<Utc>) -> Option<f64> {
        let duration = self.duration_mins? * 60_000.0;
        if duration <= 0.0 {
            return None;
        }
        Some(
            ((duration - (self.resets_at?.timestamp_millis() - now.timestamp_millis()) as f64)
                / duration)
                .clamp(0.0, 1.0),
        )
    }
    pub fn even_pace_left(&self, now: DateTime<Utc>) -> Option<i64> {
        self.elapsed_fraction(now)
            .map(|n| rounded((1.0 - n) * 100.0))
    }
}
fn pace(used: f64, elapsed: f64) -> Pace {
    let delta = used - elapsed * 100.0;
    if delta > 5.0 {
        Pace::Ahead
    } else if delta < -5.0 {
        Pace::Under
    } else {
        Pace::On
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub driver: String,
    pub enabled: bool,
    pub installed: bool,
    pub available: bool,
    pub display_name: Option<String>,
    pub authenticated: bool,
    pub plan: Option<String>,
    pub email: Option<String>,
    pub workspace: Option<String>,
    pub limits: Option<RawLimits>,
    pub is_source: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawLimits {
    pub checked_at: DateTime<Utc>,
    pub windows: Vec<Window>,
    pub fingerprint: Option<String>,
    pub credits: Option<ResetCredits>,
    pub external: Option<(String, String)>,
    pub reason: Option<String>,
    pub message: Option<String>,
}
impl RawLimits {
    fn notice(&self) -> Option<String> {
        match self.reason.as_deref() {
            Some("unsupported") => Some(
                self.message
                    .clone()
                    .unwrap_or_else(|| "This account has no subscription limits.".into()),
            ),
            Some("probeFailed") => Some(
                self.message
                    .clone()
                    .unwrap_or_else(|| "Could not read limits.".into()),
            ),
            _ if self.windows.is_empty() => Some("No limits reported.".into()),
            _ => None,
        }
    }
}
pub fn parse_record(v: &Value, fallback: &str) -> Option<Record> {
    let driver = v.get("driver")?.as_str()?.to_owned();
    if driver.is_empty() {
        return None;
    }
    let auth = v.get("auth");
    let limits = v.get("usageLimits").filter(|x| x.is_object()).map(|l| {
        let windows = l
            .get("windows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|w| {
                let id = w.get("id")?.as_str()?.to_owned();
                let used = w.get("usedPercent")?.as_f64()?;
                Some(Window {
                    label: string(w.get("label")).unwrap_or_else(|| id.clone()),
                    id,
                    kind: match w.get("kind").and_then(Value::as_str) {
                        Some("session") => WindowKind::Session,
                        Some("weekly") => WindowKind::Weekly,
                        Some("monthly") => WindowKind::Monthly,
                        _ => WindowKind::Other,
                    },
                    used_percent: used,
                    resets_at: date(w.get("resetsAt")),
                    duration_mins: w.get("windowDurationMins").and_then(Value::as_f64),
                })
            })
            .collect();
        let credits = l.get("resetCredits").and_then(|c| {
            Some(ResetCredits {
                available_count: c.get("availableCount")?.as_f64()?.clamp(0.0, 1_000_000.0) as i64,
                next_expires_at: date(c.get("nextExpiresAt")),
            })
        });
        let external = l
            .get("externalUsage")
            .and_then(|x| Some((string(x.get("label"))?, string(x.get("url"))?)));
        let unavailable = l.get("unavailable").filter(|x| x.is_object());
        RawLimits {
            checked_at: date(l.get("checkedAt")).unwrap_or(DateTime::<Utc>::MIN_UTC),
            windows,
            fingerprint: string(l.get("credentialFingerprint")),
            credits,
            external,
            reason: unavailable.map(|x| string(x.get("reason")).unwrap_or_default()),
            message: unavailable.and_then(|x| string(x.get("message"))),
        }
    });
    Some(Record {
        id: string(v.get("instanceId"))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback.into()),
        driver,
        enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        installed: v.get("installed").and_then(Value::as_bool).unwrap_or(false),
        available: v.get("availability").and_then(Value::as_str) != Some("unavailable"),
        display_name: string(v.get("displayName"))
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        authenticated: auth.and_then(|a| a.get("status")).and_then(Value::as_str)
            == Some("authenticated"),
        plan: auth
            .and_then(|a| string(a.get("label")))
            .filter(|s| !s.is_empty()),
        email: auth.and_then(|a| string(a.get("email"))),
        workspace: auth.and_then(|a| string(a.get("workspaceId"))),
        limits,
        is_source: false,
    })
}
/// A single environment's quota reports before account reconciliation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitsContribution {
    pub environment_id: EnvironmentId,
    pub records: Vec<Record>,
    pub notices: Vec<String>,
    pub skipped_files: usize,
}

/// Select environments before reconciling account ownership and constructing pools.
pub fn merge_contributions(
    contributions: &[LimitsContribution],
    selection: &EnvironmentSelection,
) -> LimitsSnapshot {
    let selected = contributions.iter().filter(|c| match selection {
        EnvironmentSelection::All => true,
        EnvironmentSelection::One(id) => &c.environment_id == id,
    });
    let mut records = Vec::new();
    let mut notices = Vec::new();
    let mut skipped = 0;
    for contribution in selected {
        skipped += contribution.skipped_files;
        notices.extend(contribution.notices.iter().cloned());
        for record in &contribution.records {
            let mut record = record.clone();
            // IDs are local to a source. Length prefixes prevent delimiter collisions.
            record.id = format!(
                "{}:{}:{}:{}:{}",
                contribution.environment_id.as_str().len(),
                contribution.environment_id,
                if record.is_source { "hub" } else { "native" },
                record.id.len(),
                record.id
            );
            records.push(record);
        }
    }
    let mut snapshot = merge(&records, skipped);
    snapshot.notices.splice(0..0, notices);
    if !snapshot.notices.is_empty() {
        snapshot.state = LimitsState::Ok;
    }
    snapshot
}

fn base_key(r: &Record) -> Option<String> {
    if let Some(email) = r.email.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(format!(
            "email:{}:{}:{}",
            r.driver.len(),
            r.driver,
            email.to_lowercase()
        ))
    } else {
        r.limits
            .as_ref()?
            .fingerprint
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|f| format!("credential:{}:{}:{}", r.driver.len(), r.driver, f))
    }
}
fn merge(records: &[Record], skipped: usize) -> LimitsSnapshot {
    let mut workspaces: HashMap<String, HashSet<String>> = HashMap::new();
    for r in records.iter().filter(|r| r.enabled) {
        if let (Some(k), Some(w)) = (base_key(r), r.workspace.as_ref().filter(|w| !w.is_empty())) {
            workspaces.entry(k).or_default().insert(w.clone());
        }
    }
    let mut merged: Vec<(String, Record)> = Vec::new();
    let mut notices = Vec::new();
    let mut links: Vec<Link> = Vec::new();
    let mut plans = HashMap::new();
    let mut credits_by_key = HashMap::new();
    for r in records
        .iter()
        .filter(|r| r.enabled && r.installed && r.available && r.limits.is_some())
    {
        let l = r.limits.as_ref().unwrap();
        if !r.is_source && l.reason.as_deref() != Some("unsupported") {
            if let Some(n) = l.notice() {
                notices.push(format!(
                    "{}: {}",
                    r.display_name.as_deref().unwrap_or(&r.driver),
                    n
                ));
            }
        }
        if r.authenticated {
            if let Some((label, url)) = &l.external {
                let name = r.display_name.as_ref().unwrap_or(&r.id).clone();
                if let Some(link) = links.iter_mut().find(|x| x.url == *url) {
                    link.label = label.clone();
                    link.message = l.message.clone();
                    if !link.accounts.contains(&name) {
                        link.accounts.push(name);
                    }
                } else {
                    links.push(Link {
                        label: label.clone(),
                        url: url.clone(),
                        message: l.message.clone(),
                        accounts: vec![name],
                    });
                }
            }
        }
        if l.notice().is_some() {
            continue;
        }
        let key = base_key(r)
            .and_then(|base| {
                if let Some(w) = r.workspace.as_ref().filter(|w| !w.is_empty()) {
                    Some(format!("known:{}:{base}:{}:{w}", base.len(), w.len()))
                } else if let Some(ws) = workspaces.get(&base) {
                    if ws.len() == 1 {
                        let w = ws.iter().next().unwrap();
                        Some(format!("known:{}:{base}:{}:{w}", base.len(), w.len()))
                    } else {
                        None
                    }
                } else {
                    Some(format!("known:{}:{base}:none", base.len()))
                }
            })
            .unwrap_or_else(|| format!("local:{}:{}", r.id.len(), r.id));
        if let Some(plan) = &r.plan {
            let entry = plans
                .entry(key.clone())
                .or_insert((l.checked_at, plan.clone()));
            if l.checked_at > entry.0 {
                *entry = (l.checked_at, plan.clone());
            }
        }
        if let Some(credits) = &l.credits {
            let entry = credits_by_key
                .entry(key.clone())
                .or_insert((l.checked_at, credits.clone()));
            if l.checked_at > entry.0 {
                *entry = (l.checked_at, credits.clone());
            }
        }
        if let Some((_, old)) = merged.iter_mut().find(|(k, _)| *k == key) {
            let ol = old.limits.as_ref().unwrap();
            let newer = l.checked_at > ol.checked_at;
            if newer {
                old.limits = Some(l.clone());
            }
            old.plan = plans.get(&key).map(|x| x.1.clone());
            old.limits.as_mut().unwrap().credits = credits_by_key.get(&key).map(|x| x.1.clone());
            if old.display_name.is_none() {
                old.display_name = r.display_name.clone();
            }
            if r.is_source
                && r.email.is_some()
                && old.display_name.as_ref().is_some_and(|n| {
                    n.eq_ignore_ascii_case(match r.driver.as_str() {
                        "codex" => "Codex",
                        "claude" | "claudeCode" => "Claude",
                        _ => &r.driver,
                    })
                })
            {
                old.display_name = None;
            }
        } else {
            merged.push((key, r.clone()));
        }
    }
    let mut ranked: Vec<usize> = (0..merged.len()).collect();
    ranked.sort_by_key(|&i| {
        let r = &merged[i].1;
        (
            r.display_name
                .as_ref()
                .or(r.email.as_ref())
                .unwrap_or(&r.id)
                .to_lowercase(),
            r.id.to_lowercase(),
        )
    });
    let initials = |r: &Record| {
        r.email.as_ref().map(|e| {
            e.split('@')
                .take(2)
                .filter_map(|s| s.chars().next())
                .flat_map(char::to_uppercase)
                .collect::<String>()
        })
    };
    let accounts = merged
        .iter()
        .enumerate()
        .map(|(i, (_, r))| {
            let same: Vec<usize> = ranked
                .iter()
                .copied()
                .filter(|&j| merged[j].1.driver == r.driver)
                .collect();
            let abbr = initials(r);
            let dup: Vec<usize> = same
                .iter()
                .copied()
                .filter(|&j| merged[j].1.display_name.is_none() && initials(&merged[j].1) == abbr)
                .collect();
            let display_name = r.display_name.clone().or_else(|| {
                if same.len() > 1 {
                    abbr.map(|a| {
                        if dup.len() > 1 {
                            format!(
                                "{} {}",
                                a,
                                dup.iter().position(|&j| j == i).unwrap_or(0) + 1
                            )
                        } else {
                            a
                        }
                    })
                } else {
                    None
                }
            });
            let l = r.limits.as_ref().unwrap();
            Account {
                id: r.id.clone(),
                driver: r.driver.clone(),
                display_name,
                plan: r.plan.clone(),
                checked_at: l.checked_at,
                windows: l.windows.clone(),
                reset_credits: l.credits.clone(),
                name_order: ranked.iter().position(|&j| j == i).unwrap(),
            }
        })
        .collect::<Vec<_>>();
    let state = if accounts.is_empty() && notices.is_empty() && links.is_empty() {
        LimitsState::NoLimitsReported
    } else {
        LimitsState::Ok
    };
    LimitsSnapshot {
        accounts,
        notices,
        links,
        state,
        skipped_files: skipped,
    }
}
/// Stateful reader. Pass the T3 state directory; caches are its sibling.
#[derive(Default)]
pub struct LimitsReader {
    entries: HashMap<String, (u64, std::time::SystemTime, Option<Record>)>,
}
impl LimitsReader {
    pub fn new() -> Self {
        Self::default()
    }
    /// Return cached reports last loaded by `read`, without reconciling accounts.
    pub fn contribution(&self, environment_id: EnvironmentId) -> LimitsContribution {
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by_key(|(name, _)| *name);
        let records: Vec<_> = entries
            .iter()
            .filter_map(|(_, entry)| entry.2.clone())
            .collect();
        LimitsContribution {
            environment_id,
            skipped_files: entries.len() - records.len(),
            records,
            notices: Vec::new(),
        }
    }
    pub fn read(&mut self, state_dir: &Path, _now: DateTime<Utc>) -> io::Result<LimitsSnapshot> {
        let dir = state_dir.parent().unwrap_or(state_dir).join("caches");
        let listing = match fs::read_dir(dir) {
            Ok(x) => x,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.entries.clear();
                return Ok(LimitsSnapshot {
                    accounts: vec![],
                    notices: vec![],
                    links: vec![],
                    state: LimitsState::NoCacheDirectory,
                    skipped_files: 0,
                });
            }
            Err(e) => return Err(e),
        };
        let mut files = Vec::new();
        for item in listing {
            let item = item?;
            let p = item.path();
            if !p
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            let m = item.metadata()?;
            if m.is_file() {
                files.push((item.file_name().to_string_lossy().into_owned(), p, m));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        self.entries.retain(|k, _| files.iter().any(|f| &f.0 == k));
        for (name, path, meta) in &files {
            let modified = meta.modified()?;
            if self
                .entries
                .get(name)
                .is_some_and(|e| e.0 == meta.len() && e.1 == modified)
            {
                continue;
            }
            if let Ok(data) = fs::read(path) {
                let parsed = serde_json::from_slice::<Value>(&data).ok().and_then(|v| {
                    parse_record(&v, path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
                });
                let old = self.entries.get(name).and_then(|e| e.2.clone());
                self.entries
                    .insert(name.clone(), (meta.len(), modified, parsed.or(old)));
            }
        }
        let records: Vec<_> = files
            .iter()
            .filter_map(|f| self.entries.get(&f.0).and_then(|e| e.2.clone()))
            .collect();
        Ok(merge(&records, files.len() - records.len()))
    }
}
fn pool_key(driver: &str, w: &Window) -> String {
    let kind = match w.kind {
        WindowKind::Session => "session",
        WindowKind::Weekly => "weekly",
        WindowKind::Monthly => "monthly",
        WindowKind::Other => "other",
    };
    if driver == "codex" {
        kind.into()
    } else {
        format!("{kind}:{}", w.id)
    }
}
fn resolve(account: &Account, window: &Window, now: DateTime<Utc>) -> Member {
    let exhausted = if window.kind == WindowKind::Session {
        account.windows.iter().find(|w| {
            w.used_percent >= 100.0
                && if account.driver == "claudeAgent" {
                    w.id == "seven_day"
                } else {
                    account.driver == "codex" && w.kind == WindowKind::Weekly
                }
        })
    } else {
        None
    };
    let mut shown = window.clone();
    let mut refill = window.used_percent;
    if let Some(weekly) = exhausted {
        shown.used_percent = 100.0;
        shown.duration_mins = None;
        shown.resets_at = None;
        refill = 0.0;
        if let (Some(w), Some(s)) = (weekly.resets_at, window.resets_at) {
            let floor = now.max(account.checked_at);
            if w > floor && s > floor {
                if s <= w {
                    shown.resets_at = Some(w);
                    refill = 100.0;
                } else if window.used_percent >= 100.0 {
                    shown.resets_at = Some(s);
                    refill = 100.0;
                } else {
                    shown.resets_at = Some(w);
                    refill = 100.0 - window.used_percent.clamp(0.0, 100.0);
                }
            }
        }
    }
    Member {
        account: account.clone(),
        window: shown,
        refill_percent: refill,
    }
}
impl LimitsSnapshot {
    pub fn groups(&self, now: DateTime<Utc>) -> Vec<Group> {
        let mut drivers = Vec::<String>::new();
        for a in &self.accounts {
            if !drivers.contains(&a.driver) {
                drivers.push(a.driver.clone());
            }
        }
        drivers
            .into_iter()
            .map(|driver| {
                let mut accounts: Vec<Account> = self
                    .accounts
                    .iter()
                    .filter(|a| a.driver == driver)
                    .cloned()
                    .collect();
                let first = accounts
                    .iter()
                    .flat_map(|a| &a.windows)
                    .min_by_key(|w| w.kind)
                    .cloned();
                accounts.sort_by_key(|a| {
                    let reset = first
                        .as_ref()
                        .and_then(|f| {
                            a.windows
                                .iter()
                                .find(|w| pool_key(&driver, w) == pool_key(&driver, f))
                        })
                        .and_then(|w| resolve(a, w, now).window.resets_at);
                    (reset.is_none(), reset, a.name_order)
                });
                let mut pools: Vec<(String, Vec<Member>)> = Vec::new();
                for a in &accounts {
                    for w in &a.windows {
                        let member = resolve(a, w, now);
                        let shared = pool_key(&driver, &member.window);
                        let duplicate = pools
                            .iter()
                            .find(|(k, _)| *k == shared)
                            .is_some_and(|(_, v)| v.iter().any(|m| m.account.id == a.id));
                        let key = if duplicate {
                            pool_key("", &member.window)
                        } else {
                            shared
                        };
                        if let Some((_, members)) = pools.iter_mut().find(|(k, _)| *k == key) {
                            members.push(member);
                        } else {
                            pools.push((key, vec![member]));
                        }
                    }
                }
                let mut rows: Vec<Row> = pools
                    .into_iter()
                    .map(|(key, members)| {
                        let head = &members[0].window;
                        let count = members.len() as f64;
                        let used =
                            members.iter().map(|m| m.window.used_percent).sum::<f64>() / count;
                        let paced: Vec<_> = members
                            .iter()
                            .filter_map(|m| {
                                m.window
                                    .elapsed_fraction(now)
                                    .map(|e| (m.window.used_percent, e))
                            })
                            .collect();
                        let row_pace = if paced.is_empty() {
                            None
                        } else {
                            Some(pace(
                                paced.iter().map(|x| x.0).sum::<f64>() / paced.len() as f64,
                                paced.iter().map(|x| x.1).sum::<f64>() / paced.len() as f64,
                            ))
                        };
                        let mut resets: Vec<Reset> = members
                            .iter()
                            .filter_map(|m| {
                                m.window.resets_at.map(|at| Reset {
                                    account_id: m.account.id.clone(),
                                    at,
                                    restores_percent: rounded(m.refill_percent / count),
                                })
                            })
                            .collect();
                        resets.sort_by_key(|r| r.at);
                        let columns = accounts
                            .iter()
                            .map(|a| Column {
                                account: a.clone(),
                                member: members.iter().find(|m| m.account.id == a.id).cloned(),
                            })
                            .collect();
                        Row {
                            id: if driver == "codex" {
                                key
                            } else {
                                head.id.clone()
                            },
                            kind: head.kind,
                            label: head.label.clone(),
                            used_percent: rounded(used),
                            remaining_percent: rounded(100.0 - used),
                            pace: row_pace,
                            resets,
                            columns,
                            members,
                        }
                    })
                    .collect();
                rows.sort_by_key(|r| r.kind);
                if driver == "cursor" {
                    let both = rows.iter().any(|r| r.id == "autoPercentUsed")
                        && rows.iter().any(|r| r.id == "apiPercentUsed");
                    if both {
                        rows.retain(|r| r.id != "totalPercentUsed");
                    }
                    rows.sort_by_key(|r| match r.id.as_str() {
                        "totalPercentUsed" => 0,
                        "autoPercentUsed" => 1,
                        "apiPercentUsed" => 2,
                        _ => 3,
                    });
                }
                if driver == "antigravity" {
                    fn stem(s: &str) -> &str {
                        s.strip_suffix("-5h")
                            .or_else(|| s.strip_suffix("-weekly"))
                            .or_else(|| s.strip_suffix("-quota"))
                            .unwrap_or(s)
                    }
                    rows.sort_by(|a, b| {
                        (
                            !a.id.starts_with("gemini-"),
                            stem(&a.id).to_lowercase(),
                            a.kind,
                        )
                            .cmp(&(
                                !b.id.starts_with("gemini-"),
                                stem(&b.id).to_lowercase(),
                                b.kind,
                            ))
                    });
                }
                Group {
                    driver,
                    accounts,
                    rows,
                }
            })
            .collect()
    }
}

impl LimitsSnapshot {
    /// Export the sanitized fixture projection used by the cross-reader oracle.
    pub fn fixture_export(&self, now: DateTime<Utc>) -> Value {
        let accounts: Vec<Value> = self
            .accounts
            .iter()
            .map(|a| {
                let mut item = serde_json::json!({
                    "id": a.id, "driver": a.driver,
                    "checkedAt": a.checked_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    "windows": a.windows.iter().map(|w| serde_json::json!({
                        "id": w.id, "usedPercent": w.used_percent, "leftPercent": w.left_percent()
                    })).collect::<Vec<_>>()
                });
                if let Some(plan) = &a.plan {
                    item["plan"] = Value::String(plan.clone());
                }
                item
            })
            .collect();
        let groups: Vec<Value> = self.groups(now).iter().map(|g| serde_json::json!({
            "driver": g.driver,
            "rows": g.rows.iter().map(|r| {
                let mut row = serde_json::json!({"id":r.id,"usedPercent":r.used_percent,"remainingPercent":r.remaining_percent});
                if let Some(pace) = r.pace { row["pace"] = serde_json::to_value(pace).expect("pace serializes"); }
                row
            }).collect::<Vec<_>>()
        })).collect();
        serde_json::json!({"schemaVersion":1,"snapshot":{"accounts":accounts,"groups":groups,"notices":self.notices,"skippedFiles":self.skipped_files}})
    }
}

mod live;
pub use live::*;
