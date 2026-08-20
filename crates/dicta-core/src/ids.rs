use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use std::{fmt, ops::Deref, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidId {
    kind: &'static str,
}

impl InvalidId {
    fn new(kind: &'static str) -> Self {
        Self { kind }
    }
}

impl fmt::Display for InvalidId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid {} identifier", self.kind)
    }
}

impl std::error::Error for InvalidId {}

fn valid_file_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !matches!(value, "." | "..")
        && !value.contains(['/', '\\', '\0'])
}

macro_rules! persisted_id {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
                let value = value.into();
                if valid_file_component(&value) {
                    Ok(Self(value))
                } else {
                    Err(InvalidId::new($kind))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidId;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(D::Error::custom)
            }
        }
    };
}

persisted_id!(ProjectId, "project");
persisted_id!(RecordingId, "recording");
persisted_id!(AnnotationId, "annotation");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_single_safe_path_components() {
        assert!(ProjectId::new("project-123").is_ok());
        assert!(RecordingId::new("20260820-12-00-00").is_ok());
        for invalid in ["", ".", "..", "../escape", "a/b", "a\\b"] {
            assert!(ProjectId::new(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn serde_keeps_ids_as_json_strings() {
        let id = ProjectId::new("demo").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"demo\"");
        assert_eq!(serde_json::from_str::<ProjectId>("\"demo\"").unwrap(), id);
        assert!(serde_json::from_str::<ProjectId>("\"../demo\"").is_err());
    }
}
