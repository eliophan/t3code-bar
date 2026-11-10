use crate::{Record, Speed};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenRates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}
impl TokenRates {
    fn scale(self, n: f64) -> Self {
        Self {
            input: self.input * n,
            output: self.output * n,
            cache_read: self.cache_read * n,
            cache_write: self.cache_write * n,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelRate {
    pub standard: TokenRates,
    pub fast: Option<TokenRates>,
    pub ultrafast: Option<TokenRates>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Categories {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub output: f64,
}
impl Categories {
    fn total(self) -> f64 {
        self.input + self.cache_read + self.cache_write + self.output
    }
    fn scale(self, n: f64) -> Self {
        Self {
            input: self.input * n,
            cache_read: self.cache_read * n,
            cache_write: self.cache_write * n,
            output: self.output * n,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricedRecord {
    pub cost_usd: f64,
    pub unpriced: bool,
    pub provider_reported: bool,
    pub categories: Option<Categories>,
    pub speed_premium_usd: f64,
    pub cache_savings_usd: f64,
}
#[derive(Debug, Clone, Default)]
pub struct PriceBook {
    rates: HashMap<String, ModelRate>,
    overrides: HashMap<String, ModelRate>,
    aliases: HashMap<String, String>,
}
fn token_rates(v: &Value, suffix: &str, standard: Option<TokenRates>) -> Option<TokenRates> {
    let get = |name: &str| v.get(format!("{name}{suffix}")).and_then(Value::as_f64);
    let input = get("input_cost_per_token")?;
    let output = get("output_cost_per_token")?;
    let cache = |name: &str, old: Option<f64>| {
        get(name).unwrap_or_else(|| match (standard, old) {
            (Some(s), Some(n)) if s.input > 0.0 => n / s.input * input,
            _ => input,
        })
    };
    Some(TokenRates {
        input,
        output,
        cache_read: cache(
            "cache_read_input_token_cost",
            standard.map(|s| s.cache_read),
        ),
        cache_write: cache(
            "cache_creation_input_token_cost",
            standard.map(|s| s.cache_write),
        ),
    })
}
impl PriceBook {
    pub(crate) fn known_models(&self) -> usize {
        self.rates.len()
    }
    pub fn load(directory: &Path) -> Self {
        let read = |name| {
            std::fs::read(directory.join(name))
                .ok()
                .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
                .unwrap_or(Value::Null)
        };
        Self::from_documents(&read("usage-model-rates.json"), &read("settings.json"))
    }
    pub fn from_documents(table: &Value, settings: &Value) -> Self {
        let mut result = Self::default();
        let mut order = Vec::new();
        if let Some(doc) = table.get("document").and_then(Value::as_object) {
            for (name, v) in doc {
                let Some(standard) = token_rates(v, "", None) else {
                    continue;
                };
                let key = name.trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                let fast = v
                    .pointer("/provider_specific_entry/fast")
                    .and_then(Value::as_f64)
                    .filter(|n| *n > 0.0)
                    .map(|n| standard.scale(n))
                    .or_else(|| token_rates(v, "_priority", Some(standard)));
                let rate = ModelRate {
                    standard,
                    fast,
                    ultrafast: token_rates(v, "_ultrafast", Some(standard)),
                };
                if !result.rates.contains_key(&key) {
                    order.push(key.clone());
                }
                result.rates.insert(key, rate);
            }
        }
        let first_party = [
            "anthropic",
            "deepseek",
            "gemini",
            "meta",
            "mistral",
            "openai",
            "xai",
        ];
        let mut candidates: HashMap<String, (Option<ModelRate>, bool)> = HashMap::new();
        for key in order {
            let Some((provider, _)) = key.split_once('/') else {
                continue;
            };
            let alias = key.rsplit('/').next().unwrap_or("");
            if alias.is_empty() || result.rates.contains_key(alias) {
                continue;
            }
            let rate = result.rates[&key];
            let first = first_party.contains(&provider);
            match candidates.get_mut(alias) {
                None => {
                    candidates.insert(alias.to_owned(), (Some(rate), first));
                }
                Some((held, is_first)) if first && !*is_first => {
                    *held = Some(rate);
                    *is_first = true;
                }
                Some((held, is_first)) if first == *is_first => {
                    if let Some(a) = *held {
                        let fast = a.fast.or(rate.fast);
                        let ultra = a.ultrafast.or(rate.ultrafast);
                        *held = if a.standard == rate.standard
                            && fast == rate.fast.or(fast)
                            && ultra == rate.ultrafast.or(ultra)
                        {
                            Some(ModelRate {
                                standard: a.standard,
                                fast,
                                ultrafast: ultra,
                            })
                        } else {
                            None
                        };
                    }
                }
                _ => (),
            }
        }
        for (alias, (rate, _)) in candidates {
            if let Some(rate) = rate {
                result.rates.insert(alias, rate);
            }
        }
        if let Some(overrides) = settings
            .get("usagePriceOverrides")
            .and_then(Value::as_object)
        {
            for (name, v) in overrides {
                let (Some(input), Some(output)) = (
                    v.get("inputCostPerMillionTokens").and_then(Value::as_f64),
                    v.get("outputCostPerMillionTokens").and_then(Value::as_f64),
                ) else {
                    continue;
                };
                let standard = TokenRates {
                    input: input / 1e6,
                    output: output / 1e6,
                    cache_read: v
                        .get("cacheReadCostPerMillionTokens")
                        .and_then(Value::as_f64)
                        .unwrap_or(input)
                        / 1e6,
                    cache_write: v
                        .get("cacheWriteCostPerMillionTokens")
                        .and_then(Value::as_f64)
                        .unwrap_or(input)
                        / 1e6,
                };
                result.overrides.insert(
                    name.trim().into(),
                    ModelRate {
                        standard,
                        fast: None,
                        ultrafast: None,
                    },
                );
            }
        }
        if let Some(raw) = settings.get("usageModelAliases").and_then(Value::as_object) {
            let raw: HashMap<&str, &str> = raw
                .iter()
                .filter_map(|(k, v)| Some((k.as_str(), v.as_str()?)))
                .collect();
            for &name in raw.keys() {
                let mut seen = HashSet::from([name]);
                let mut target = raw[name];
                while let Some(&next) = raw.get(target) {
                    if !seen.insert(target) {
                        break;
                    }
                    target = next;
                }
                if !raw.contains_key(target) {
                    result.aliases.insert(name.into(), target.into());
                }
            }
        }
        result
    }
    pub fn alias(&self, model: &str) -> Option<&str> {
        self.aliases.get(model).map(String::as_str)
    }
    pub fn rate(&self, model: &str) -> Option<ModelRate> {
        let name = model.trim().to_lowercase();
        let key = name.split('[').next().unwrap_or("");
        let bare = key.rsplit('/').next().unwrap_or("");
        if [
            "",
            "<synthetic>",
            "synthetic",
            "opus",
            "sonnet",
            "haiku",
            "fable",
        ]
        .contains(&bare)
        {
            None
        } else {
            self.rates.get(key).copied()
        }
    }
    pub fn price(&self, r: &Record) -> PricedRecord {
        let override_rate = self.overrides.get(r.model.trim()).copied();
        let rate = override_rate.or_else(|| {
            self.rate(
                r.rate_model
                    .as_deref()
                    .filter(|n| !n.is_empty())
                    .unwrap_or(&r.model),
            )
        });
        let reported = if override_rate.is_some() {
            None
        } else {
            r.reported_cost_usd.filter(|n| n.is_finite())
        };
        let Some(rate) = rate else {
            return PricedRecord {
                cost_usd: reported.unwrap_or(0.0),
                unpriced: reported.is_none(),
                provider_reported: reported.is_some(),
                categories: None,
                speed_premium_usd: 0.0,
                cache_savings_usd: 0.0,
            };
        };
        let active = match r.speed {
            Speed::Standard => rate.standard,
            Speed::Fast => rate.fast.unwrap_or(rate.standard),
            Speed::Ultrafast => rate.ultrafast.unwrap_or(rate.standard),
        };
        let categories = |t: TokenRates| Categories {
            input: r.uncached_input as f64 * t.input,
            cache_read: r.cached_input as f64 * t.cache_read,
            cache_write: r.cache_creation as f64 * t.cache_write,
            output: r.output as f64 * t.output,
        };
        let list = categories(active);
        let total = list.total();
        let savings = r.cached_input as f64 * (active.input - active.cache_read);
        if let Some(cost) = reported {
            if total <= 0.0 {
                return PricedRecord {
                    cost_usd: cost,
                    unpriced: false,
                    provider_reported: true,
                    categories: None,
                    speed_premium_usd: 0.0,
                    cache_savings_usd: savings,
                };
            }
        }
        let scale = reported.map(|n| n / total).unwrap_or(1.0);
        PricedRecord {
            cost_usd: reported.unwrap_or(total),
            unpriced: false,
            provider_reported: reported.is_some(),
            categories: Some(list.scale(scale)),
            speed_premium_usd: if r.speed == Speed::Standard {
                0.0
            } else {
                (total - categories(rate.standard).total()) * scale
            },
            cache_savings_usd: savings,
        }
    }
}
