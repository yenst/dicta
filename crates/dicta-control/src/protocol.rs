use crate::{ErrorCode, ProtocolError};
use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroU64};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RequestEnvelope {
    pub version: u16,
    pub id: RequestId,
    #[serde(flatten)]
    pub command: Command,
}

impl RequestEnvelope {
    pub const fn new(id: RequestId, command: Command) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            command,
        }
    }

    pub fn validate_version(&self) -> Result<(), ProtocolError> {
        if self.version == PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(ProtocolError::new(
                ErrorCode::UnsupportedVersion,
                format!(
                    "protocol version {} is unsupported; expected {PROTOCOL_VERSION}",
                    self.version
                ),
            ))
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ResponseEnvelope {
    pub version: u16,
    pub id: RequestId,
    #[serde(flatten)]
    pub payload: ResponsePayload,
}

impl ResponseEnvelope {
    pub const fn success(id: RequestId, result: Response) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            payload: ResponsePayload::Success { result },
        }
    }

    pub const fn failure(id: RequestId, error: ProtocolError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            payload: ResponsePayload::Failure { error },
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ResponsePayload {
    Success { result: Response },
    Failure { error: ProtocolError },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EventEnvelope {
    pub version: u16,
    #[serde(flatten)]
    pub event: Event,
}

impl EventEnvelope {
    pub const fn new(event: Event) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            event,
        }
    }
}

/// A server frame is either correlated to a request or is an unsolicited event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ServerMessage {
    Response(ResponseEnvelope),
    Event(EventEnvelope),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "command", content = "params", rename_all = "snake_case")]
pub enum Command {
    Status,
    UiShow,
    SettingsGet,
    SettingsSetShortcut {
        shortcut_id: String,
    },
    SettingsSetCleanup {
        enabled: bool,
    },
    SettingsSetBranchLocking {
        enabled: bool,
    },
    SettingsSetLanguage {
        language: String,
    },
    SettingsSetGeneralPath {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    SettingsCleanupMerged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
    },
    ModelStatus,
    ModelInstall {
        model: ModelTier,
    },
    Events {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since_sequence: Option<u64>,
    },
    ProjectList,
    ProjectAdd {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    ProjectCreate {
        name: String,
    },
    ProjectRemove {
        project: String,
    },
    ProjectRefresh {
        project: String,
    },
    ProjectSelect {
        project: String,
    },
    ProjectCurrent,
    RecordingList {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    RecordingShow {
        recording: RecordingSelector,
    },
    RecordingOpen {
        recording: RecordingSelector,
    },
    RecordingTranscribe {
        recording: RecordingSelector,
    },
    RecordingSetTimelineNotes {
        recording: RecordingSelector,
        notes: Vec<dicta_core::TimelineNote>,
    },
    RecordingVoiceNoteTranscribe {
        recording: RecordingSelector,
        note_id: String,
        timestamp_seconds: f64,
        audio_path: String,
    },
    RecordingVoiceNoteCancel,
    RecordingVoiceNoteStatus,
    RecordingDelete {
        recording: RecordingSelector,
    },
    Context {
        recording: RecordingSelector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default)]
        copy: bool,
    },
    RecordStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    RecordStop,
    RecordToggle,
    RecordStatus,
    AnnotationToggle,
    AnnotationEnable,
    AnnotationDisable,
    AnnotationTool {
        tool: AnnotationTool,
    },
    AnnotationUndo,
    AnnotationClear,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum RecordingSelector {
    Id(String),
    Latest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationTool {
    Pen,
    Arrow,
    Rectangle,
    Spotlight,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Quality,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Accepted,
    Settings(dicta_core::storage::AppSettings),
    Cleanup(CleanupSummary),
    ModelInstallStarted,
    Status(StatusSnapshot),
    ModelStatus(ModelStatusSummary),
    Projects(Vec<ProjectSummary>),
    Project(Option<ProjectSummary>),
    Recordings(Vec<RecordingSummary>),
    Recording(RecordingSummary),
    RecordingDetails(Box<dicta_core::RecordingFile>),
    VoiceNote(VoiceNoteStatus),
    Context { text: String },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceNoteState {
    #[default]
    Idle,
    Processing,
    Complete,
    Failed,
    Cancelling,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct VoiceNoteStatus {
    pub state: VoiceNoteState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CleanupSummary {
    pub removed_files: usize,
    pub freed_bytes: u64,
    pub cleaned_branches: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelStatusSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model_path: Option<String>,
    pub quality_state: ModelState,
    pub quality_path: String,
    pub quality_size_bytes: u64,
    pub expected_download_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_stage: Option<ModelInstallStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded_bytes: Option<u64>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    Missing,
    Partial,
    Ready,
    Invalid,
    Unverified,
    Installing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelInstallStage {
    Locating,
    Downloading,
    Verifying,
    Ready,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatusSnapshot {
    pub phase: AppPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,
    pub annotations_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation_tool: Option<AnnotationTool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppPhase {
    Idle,
    Preparing,
    Recording,
    Stopping,
    Transcribing,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecordingSummary {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_preview: Option<String>,
    #[serde(default)]
    pub success: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub recording_scope: String,
    #[serde(default)]
    pub timeline_note_count: u32,
    #[serde(default)]
    pub has_annotations: bool,
    pub duration_seconds: f64,
    pub transcription: TranscriptionState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionState {
    Pending,
    Processing,
    Complete,
    Failed,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    UiShowRequested {
        sequence: u64,
    },
    UiRecordingRequested {
        sequence: u64,
        recording_id: String,
    },
    StateChanged {
        sequence: u64,
        status: StatusSnapshot,
    },
    RecordingStarted {
        sequence: u64,
        recording_id: String,
    },
    RecordingStopped {
        sequence: u64,
        recording_id: String,
        duration_seconds: f64,
    },
    AnnotationCreated {
        sequence: u64,
        tool: AnnotationTool,
        timestamp_seconds: f64,
    },
    TranscriptionCompleted {
        sequence: u64,
        recording_id: String,
    },
    Failed {
        sequence: u64,
        error: ProtocolError,
    },
}
