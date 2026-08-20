use crate::{ProjectId, RecordingId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};
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

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct TimelineNote {
    pub id: String,
    pub timestamp_seconds: f64,
    pub text: String,
    pub created_at: DateTime<Utc>,
    #[serde(default = "typed_note_source")]
    pub source: String,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl TimelineNote {
    pub fn is_valid(&self) -> bool {
        !self.id.trim().is_empty()
            && self.timestamp_seconds.is_finite()
            && self.timestamp_seconds >= 0.0
            && !self.text.trim().is_empty()
    }
}

fn typed_note_source() -> String {
    "typed".to_string()
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RecordingFile {
    pub id: RecordingId,
    pub project_id: ProjectId,
    #[serde(default)]
    pub video_path: String,
    #[serde(default)]
    pub metadata_path: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub recording_scope: RecordingScope,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub transcription_status: TranscriptionStatus,
    #[serde(default)]
    pub transcription_error: Option<String>,
    #[serde(default)]
    pub transcription_language: Option<String>,
    #[serde(default)]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub annotation_path: Option<String>,
    #[serde(default)]
    pub timeline_notes: Vec<TimelineNote>,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl RecordingFile {
    pub fn is_valid(&self) -> bool {
        self.duration_seconds
            .is_none_or(|duration| duration.is_finite() && duration >= 0.0)
            && self
                .ended_at
                .zip(self.started_at)
                .is_none_or(|(ended, started)| ended >= started)
            && self
                .transcript_segments
                .iter()
                .all(TranscriptSegment::is_valid)
            && self.timeline_notes.iter().all(TimelineNote::is_valid)
    }
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

    #[test]
    fn current_recording_metadata_loads_without_migration() {
        let json = r##"{
          "duration_seconds": 82.254,
          "ended_at": "2026-08-19T18:20:06.638258196Z",
          "git_branch": "main",
          "id": "20260819-20-18-43",
          "metadata_path": "/repo/.dicta/recordings/20-18-43.json",
          "note": "Explain the overflow",
          "poster_path": "/repo/.dicta/recordings/20-18-43.poster.jpg",
          "project_id": "dicta",
          "recording_scope": "branch",
          "size_bytes": 12625537,
          "started_at": "2026-08-19T18:18:44.383401263Z",
          "success": true,
          "timeline_notes": [],
          "transcript": "The layout is getting squashed.",
          "transcript_segments": [{
            "end_seconds": 29.8,
            "start_seconds": 0.0,
            "text": "The layout is getting squashed."
          }],
          "transcription_language": "en",
          "transcription_status": "complete",
          "video_path": "/repo/.dicta/recordings/20-18-43.mp4",
          "future_metadata": {"preserved": true}
        }"##;

        let recording: RecordingFile = serde_json::from_str(json).unwrap();
        assert!(recording.is_valid());
        assert_eq!(recording.recording_scope, RecordingScope::Branch);
        assert_eq!(
            recording.transcription_status,
            TranscriptionStatus::Complete
        );
        assert_eq!(recording.extra["future_metadata"]["preserved"], true);

        let encoded = serde_json::to_value(recording).unwrap();
        assert_eq!(encoded["future_metadata"]["preserved"], true);
    }

    #[test]
    fn sparse_legacy_recording_metadata_uses_safe_defaults() {
        let recording: RecordingFile =
            serde_json::from_str(r#"{"id":"20260820-12-00-00","project_id":"dicta"}"#).unwrap();
        assert!(recording.is_valid());
        assert_eq!(recording.recording_scope, RecordingScope::Branch);
        assert_eq!(recording.transcription_status, TranscriptionStatus::Unknown);
        assert!(recording.transcript_segments.is_empty());
    }
}
