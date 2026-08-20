use crate::*;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Project {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) storage_path: String,
    pub(crate) source_path: Option<String>,
    pub(crate) git_branch: Option<String>,
    pub(crate) branch_path: Option<String>,
    pub(crate) is_git: bool,
    pub(crate) git_error: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) recording_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Recording {
    pub(crate) id: RecordingId,
    pub(crate) project_id: ProjectId,
    pub(crate) video_path: String,
    pub(crate) metadata_path: String,
    pub(crate) note: String,
    #[serde(default)]
    pub(crate) recording_scope: RecordingScope,
    #[serde(default)]
    pub(crate) git_branch: Option<String>,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) ended_at: Option<DateTime<Utc>>,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) success: bool,
    #[serde(default)]
    pub(crate) transcript: Option<String>,
    #[serde(default)]
    pub(crate) transcript_path: Option<String>,
    #[serde(default)]
    pub(crate) transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub(crate) transcription_status: TranscriptionStatus,
    #[serde(default)]
    pub(crate) transcription_error: Option<String>,
    #[serde(default)]
    pub(crate) transcription_language: Option<String>,
    #[serde(default)]
    pub(crate) poster_path: Option<String>,
    #[serde(default)]
    pub(crate) timeline_notes: Vec<TimelineNote>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TimelineNote {
    pub(crate) id: String,
    pub(crate) timestamp_seconds: f64,
    pub(crate) text: String,
    pub(crate) created_at: DateTime<Utc>,
    #[serde(default = "typed_note_source")]
    pub(crate) source: String,
}

pub(crate) fn typed_note_source() -> String {
    "typed".to_string()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RecordingPhase {
    Idle,
    Preparing,
    Recording,
    Stopping,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RecorderStatus {
    pub(crate) phase: RecordingPhase,
    pub(crate) active_project_id: Option<String>,
    pub(crate) active_video_path: Option<String>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Bootstrap {
    pub(crate) root_path: String,
    pub(crate) projects: Vec<Project>,
    pub(crate) status: RecorderStatus,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct McpStatus {
    pub(crate) installed: bool,
    pub(crate) codex_configured: bool,
    pub(crate) executable_path: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ModelStatus {
    pub(crate) bundled_ready: bool,
    pub(crate) quality_installed: bool,
    pub(crate) quality_path: String,
    pub(crate) quality_size_bytes: u64,
    pub(crate) download_size_bytes: u64,
    pub(crate) active_model: String,
    pub(crate) active_model_path: String,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ModelDownloadEvent {
    pub(crate) downloaded_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) progress: f64,
    pub(crate) status: String,
    pub(crate) message: String,
}

pub(crate) struct LoadedWhisper {
    pub(crate) path: PathBuf,
    pub(crate) context: WhisperContext,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct CleanupSummary {
    pub(crate) removed_files: usize,
    pub(crate) freed_bytes: u64,
    pub(crate) cleaned_branches: Vec<String>,
    pub(crate) default_branch: Option<String>,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RecorderEventPayload {
    pub(crate) event: String,
    pub(crate) message: String,
    pub(crate) status: RecorderStatus,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RecordingSelection {
    pub(crate) project_id: String,
    pub(crate) recording_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NativeTranscriptionPayload {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) transcript: Option<String>,
    #[serde(default)]
    pub(crate) transcript_segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub(crate) error: Option<String>,
}

pub(crate) struct LocalTranscript {
    pub(crate) transcript: String,
    pub(crate) segments: Vec<TranscriptSegment>,
}

pub(crate) struct InnerState {
    pub(crate) status: RecorderStatus,
    pub(crate) session: Option<Recording>,
    pub(crate) last_note: String,
    pub(crate) pending_recording_selection: Option<RecordingSelection>,
}

pub(crate) struct AppState {
    pub(crate) root: PathBuf,
    pub(crate) inner: Mutex<InnerState>,
}

impl AppState {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Mutex::new(InnerState {
                status: RecorderStatus {
                    phase: RecordingPhase::Idle,
                    active_project_id: None,
                    active_video_path: None,
                    started_at: None,
                    last_error: None,
                },
                session: None,
                last_note: String::new(),
                pending_recording_selection: None,
            }),
        }
    }
}
