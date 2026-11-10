use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ID must not be blank")
    }
}

impl std::error::Error for InvalidId {}

// ECMAScript String.trim whitespace, as used by the upstream ID schema.
fn trim_id(value: &str) -> &str {
    value.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'))
}

macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidId> {
                let value = trim_id(value.as_ref());
                if value.is_empty() {
                    return Err(InvalidId);
                }
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = InvalidId;
            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidId;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

entity_id!(EnvironmentId);
entity_id!(ThreadId);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRef {
    pub environment_id: EnvironmentId,
    pub thread_id: ThreadId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentSelection {
    #[default]
    All,
    One(EnvironmentId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum EnvironmentMachineKind {
    Server,
    Cloud,
    Linux,
    Desktop,
    Laptop,
    MacMini,
    MacStudio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentAccess {
    Direct,
    Cloud,
    DirectOrCloud,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentView {
    pub id: EnvironmentId,
    pub label: String,
    pub machine: EnvironmentMachineKind,
    pub status: String,
    pub access: EnvironmentAccess,
    /// Whether Bar holds, or could hold, its own direct or SSH grant that the
    /// user may remove. False for the local environment and Relay-only sources.
    pub can_forget_access: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_follow_upstream_trimmed_nonblank_contract() {
        assert_eq!(
            EnvironmentId::new(" \u{feff}env/local \n")
                .unwrap()
                .as_str(),
            "env/local"
        );
        assert_eq!(ThreadId::new(" thread:42 ").unwrap().as_str(), "thread:42");
        assert!(EnvironmentId::new(" \u{feff}\n").is_err());
        assert!(ThreadId::new("").is_err());
        assert_eq!(ThreadId::new("\u{85}").unwrap().as_str(), "\u{85}");
        assert!(serde_json::from_str::<ThreadId>("\"  \"").is_err());
        assert_eq!(
            serde_json::from_str::<EnvironmentId>("\" env \"")
                .unwrap()
                .as_str(),
            "env"
        );
    }

    #[test]
    fn identities_and_selections_round_trip() {
        let reference = ThreadRef {
            environment_id: EnvironmentId::new("env-a").unwrap(),
            thread_id: ThreadId::new("thread").unwrap(),
        };
        let json = serde_json::to_string(&reference).unwrap();
        assert_eq!(json, r#"{"environmentId":"env-a","threadId":"thread"}"#);
        assert_eq!(serde_json::from_str::<ThreadRef>(&json).unwrap(), reference);
        assert!(
            serde_json::from_str::<ThreadRef>(r#"{"environmentId":" ","threadId":"thread"}"#)
                .is_err()
        );
        for selection in [
            EnvironmentSelection::All,
            EnvironmentSelection::One(reference.environment_id),
        ] {
            let json = serde_json::to_string(&selection).unwrap();
            assert_eq!(
                serde_json::from_str::<EnvironmentSelection>(&json).unwrap(),
                selection
            );
        }
        assert!(serde_json::from_str::<EnvironmentSelection>(r#"{"one":" "}"#).is_err());
    }

    #[test]
    fn machine_kinds_match_upstream_wire_values() {
        for (kind, wire) in [
            (EnvironmentMachineKind::Server, "server"),
            (EnvironmentMachineKind::Cloud, "cloud"),
            (EnvironmentMachineKind::Linux, "linux"),
            (EnvironmentMachineKind::Desktop, "desktop"),
            (EnvironmentMachineKind::Laptop, "laptop"),
            (EnvironmentMachineKind::MacMini, "mac-mini"),
            (EnvironmentMachineKind::MacStudio, "mac-studio"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<EnvironmentMachineKind>(wire.into()).unwrap(),
                kind
            );
        }
        assert!(serde_json::from_str::<EnvironmentMachineKind>("\"unknown\"").is_err());
    }
}
