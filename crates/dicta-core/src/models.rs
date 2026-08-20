use crate::ProjectId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

fn unix_epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::UNIX_EPOCH)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectFile {
    pub id: ProjectId,
    pub name: String,
    #[serde(default = "unix_epoch")]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub source_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BranchMetadata {
    pub git_branch: String,
    #[serde(default)]
    pub head_oid: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingScope {
    #[default]
    Branch,
    Repository,
    Unprojected,
    #[serde(other)]
    Unknown,
}

impl RecordingScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Repository => "repository",
            Self::Unprojected => "unprojected",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RecordingScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionStatus {
    Pending,
    Processing,
    Complete,
    Failed,
    #[default]
    #[serde(other)]
    Unknown,
}

impl TranscriptionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Unknown => "",
        }
    }
}

impl fmt::Display for TranscriptionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for TranscriptionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TranscriptSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

impl TranscriptSegment {
    pub fn is_valid(&self) -> bool {
        self.start_seconds.is_finite()
            && self.end_seconds.is_finite()
            && self.start_seconds >= 0.0
            && self.end_seconds >= self.start_seconds
            && !self.text.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_enum_shapes_stay_compatible() {
        assert_eq!(
            serde_json::to_string(&RecordingScope::Repository).unwrap(),
            "\"repository\""
        );
        assert_eq!(
            serde_json::to_string(&TranscriptionStatus::Processing).unwrap(),
            "\"processing\""
        );
        assert_eq!(
            serde_json::from_str::<TranscriptionStatus>("\"\"").unwrap(),
            TranscriptionStatus::Unknown
        );
        assert_eq!(
            serde_json::to_string(&TranscriptionStatus::Unknown).unwrap(),
            "\"\""
        );
    }

    #[test]
    fn old_project_files_without_created_at_still_load() {
        let project: ProjectFile = serde_json::from_str(r#"{"id":"demo","name":"Demo"}"#).unwrap();
        assert_eq!(project.id.as_str(), "demo");
        assert_eq!(project.created_at, unix_epoch());
    }
}
