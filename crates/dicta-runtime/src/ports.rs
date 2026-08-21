//! Injected platform ports for the native runtime.

use dicta_capture::CaptureArtifact;
use dicta_control::AnnotationTool;
use dicta_control::CleanupSummary;
use dicta_control::{ErrorCode, ProtocolError};
use dicta_core::{
    storage::AppSettings, AnnotationFile, ProjectFile, ProjectId, RecordingFile, RecordingId,
    TimelineNote,
};
use dicta_engine::RecordingSession;
use dicta_transcribe::{ModelInstallOutcome, ModelPreparation, ModelStatus, TranscriptionOutput};
use std::{error::Error, fmt, time::SystemTime};

/// A port operation either completed in the caller or will complete later.
#[derive(Clone, Debug, PartialEq)]
pub enum Completion<T> {
    Ready(T),
    Pending,
}

/// One completed background transcription, keyed to its originating recording.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionCompletion {
    pub recording_id: RecordingId,
    pub result: Result<TranscriptionOutput, PortError>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelInstallPoll {
    Progress(ModelPreparation),
    Completed(Result<ModelInstallOutcome, PortError>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortErrorKind {
    InvalidRequest,
    Unavailable,
    PermissionDenied,
    NotFound,
    Conflict,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortError {
    pub kind: PortErrorKind,
    pub message: String,
}

impl PortError {
    #[must_use]
    pub fn new(kind: PortErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn protocol_error(&self) -> ProtocolError {
        let code = match self.kind {
            PortErrorKind::InvalidRequest => ErrorCode::InvalidRequest,
            PortErrorKind::Unavailable => ErrorCode::Unavailable,
            PortErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            PortErrorKind::NotFound => ErrorCode::NotFound,
            PortErrorKind::Conflict => ErrorCode::Conflict,
            PortErrorKind::Internal => ErrorCode::Internal,
        };
        ProtocolError::new(code, self.message.clone())
    }
}

impl fmt::Display for PortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PortError {}

/// Nonblocking observation of an in-flight recorder.
#[derive(Clone, Debug, PartialEq)]
pub enum CapturePoll {
    Idle,
    Running,
    Stopped(CaptureArtifact),
}

/// Starts and stops the platform recorder. Starting may include device discovery.
pub trait CapturePort {
    /// # Errors
    /// Returns a platform or device error when capture cannot be started.
    fn start(&mut self, session: &RecordingSession) -> Result<Completion<()>, PortError>;

    /// # Errors
    /// Returns a recorder or finalization error when capture cannot be stopped.
    fn stop(
        &mut self,
        session: &RecordingSession,
    ) -> Result<Completion<CaptureArtifact>, PortError>;

    /// Observes an active recorder without blocking. Implementations that never
    /// run in the background may leave the default idle result.
    ///
    /// # Errors
    /// Returns a recorder exit or finalization error when capture dies unexpectedly.
    fn poll(&mut self) -> Result<CapturePoll, PortError> {
        Ok(CapturePoll::Idle)
    }
}

/// Submits or performs transcription for one persisted recording.
pub trait TranscriptionPort {
    /// Reports whether this port can currently accept transcription work.
    fn is_available(&self) -> bool {
        true
    }

    /// # Errors
    /// Returns a model, queue, or inference error when work cannot be accepted.
    fn transcribe(
        &mut self,
        recording: &RecordingFile,
    ) -> Result<Completion<TranscriptionOutput>, PortError>;

    /// Polls once without waiting for a submitted transcription to finish.
    fn poll_completion(&mut self) -> Option<TranscriptionCompletion> {
        None
    }

    /// Returns the current local model state. Implementations may perform an
    /// explicit integrity check, but should cache a verified file identity.
    ///
    /// # Errors
    /// Returns unavailable when model management is not attached.
    fn model_status(&mut self) -> Result<ModelStatus, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "model management is not attached",
        ))
    }

    /// Starts a background quality-model installation.
    ///
    /// # Errors
    /// Returns a conflict/unavailable error when another model or transcription
    /// operation is active.
    fn install_quality_model(&mut self) -> Result<Completion<ModelInstallOutcome>, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "model installation is not attached",
        ))
    }

    /// Polls installer progress or completion without waiting.
    fn poll_model_install(&mut self) -> Option<ModelInstallPoll> {
        None
    }

    /// Applies the language used by future transcription jobs.
    ///
    /// # Errors
    /// Returns a typed error when the concrete backend cannot update its
    /// language policy.
    fn set_language(&mut self, _language: &str) -> Result<(), PortError> {
        Ok(())
    }
}

/// Owns the live overlay and its in-memory annotation document.
pub trait AnnotationPort {
    /// # Errors
    /// Returns an overlay error when its input mode cannot be changed.
    fn set_enabled(&mut self, recording_id: &RecordingId, enabled: bool) -> Result<(), PortError>;

    /// # Errors
    /// Returns an overlay error when the tool cannot be selected.
    fn set_tool(
        &mut self,
        recording_id: &RecordingId,
        tool: AnnotationTool,
    ) -> Result<(), PortError>;

    /// # Errors
    /// Returns an overlay error when the latest annotation cannot be removed.
    fn undo(&mut self, recording_id: &RecordingId) -> Result<(), PortError>;

    /// # Errors
    /// Returns an overlay error when the annotation canvas cannot be cleared.
    fn clear(&mut self, recording_id: &RecordingId) -> Result<(), PortError>;

    /// # Errors
    /// Returns an overlay error when its document cannot be finalized.
    fn finish(&mut self, recording_id: &RecordingId) -> Result<Option<AnnotationFile>, PortError>;
}

/// Persists existing core models; the runtime defines no competing file format.
pub trait StoragePort {
    /// Loads the single legacy-compatible application settings document.
    ///
    /// # Errors
    /// Returns a storage error when preferences cannot be read safely.
    fn load_settings(&mut self) -> Result<AppSettings, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "settings storage is not attached",
        ))
    }

    /// Atomically persists the single application settings document.
    ///
    /// # Errors
    /// Returns a storage error when preferences cannot be written safely.
    fn save_settings(&mut self, _settings: &AppSettings) -> Result<(), PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "settings storage is not attached",
        ))
    }

    /// Atomically replaces the complete validated timeline-note collection for
    /// one catalog recording and returns the updated shared core model.
    ///
    /// # Errors
    /// Returns a storage or confinement error when the recording cannot be
    /// resolved or its metadata cannot be replaced safely.
    fn save_timeline_notes(
        &mut self,
        _recording: &RecordingFile,
        _notes: &[TimelineNote],
    ) -> Result<RecordingFile, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "timeline-note storage is not attached",
        ))
    }

    /// Removes only video artifacts from branch packets proven merged into the
    /// repository default branch.
    ///
    /// # Errors
    /// Returns a storage or Git error when cleanup cannot be proven safe.
    fn cleanup_merged_videos(
        &mut self,
        _project_id: &ProjectId,
    ) -> Result<CleanupSummary, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "merged-video cleanup is not attached",
        ))
    }

    /// Loads registered projects for read commands.
    ///
    /// # Errors
    /// Returns a storage error when project metadata cannot be enumerated safely.
    fn load_projects(&mut self) -> Result<Vec<ProjectFile>, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "project catalog is not attached",
        ))
    }

    /// Loads recording metadata for read and explicit-transcription commands.
    ///
    /// # Errors
    /// Returns a storage error when recording metadata cannot be enumerated safely.
    fn load_recordings(&mut self) -> Result<Vec<RecordingFile>, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "recording catalog is not attached",
        ))
    }

    /// Polls one recording discovered asynchronously during startup whose
    /// pending/failed transcription can be retried.
    fn poll_transcription_retry(&mut self) -> Option<Result<RecordingFile, PortError>> {
        None
    }

    /// Registers an existing Git project and prepares repository-local storage.
    ///
    /// # Errors
    /// Returns a validation, permission, collision, or storage error.
    fn add_project(&mut self, _path: &str, _name: Option<&str>) -> Result<ProjectFile, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "project registration is not attached",
        ))
    }

    /// Creates a standalone project beneath the configured storage root.
    ///
    /// # Errors
    /// Returns a validation, collision, or storage error.
    fn create_project(&mut self, _name: &str) -> Result<ProjectFile, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "project creation is not attached",
        ))
    }

    /// Removes only a project's registration, preserving its recording data.
    ///
    /// # Errors
    /// Returns a validation, permission, not-found, or storage error.
    fn remove_project(&mut self, _project_id: &ProjectId) -> Result<(), PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "project removal is not attached",
        ))
    }

    /// Deletes the persisted artifacts belonging to a catalog recording.
    ///
    /// # Errors
    /// Returns a storage error when the recording cannot be resolved or every
    /// artifact cannot be proven safe to remove.
    fn delete_recording(&mut self, _recording: &RecordingFile) -> Result<(), PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "recording deletion is not attached",
        ))
    }

    /// # Errors
    /// Returns a storage error when recording metadata or sidecars cannot be saved.
    fn save_recording(
        &mut self,
        session: &RecordingSession,
        artifact: &CaptureArtifact,
        annotations: Option<&AnnotationFile>,
    ) -> Result<RecordingFile, PortError>;

    /// # Errors
    /// Returns a storage error when transcript output cannot be saved.
    fn save_transcription(
        &mut self,
        recording_id: &RecordingId,
        output: &TranscriptionOutput,
    ) -> Result<(), PortError>;

    /// Persists restart-safe pending state before background inference begins.
    ///
    /// # Errors
    /// Returns a storage error when the recording cannot be resolved or updated.
    fn mark_transcription_pending(&mut self, _recording_id: &RecordingId) -> Result<(), PortError> {
        Ok(())
    }

    /// Persists a retryable failed state when background inference fails.
    ///
    /// # Errors
    /// Returns a storage error when the recording cannot be resolved or updated.
    fn mark_transcription_failed(
        &mut self,
        _recording_id: &RecordingId,
        _message: &str,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

pub trait Clock {
    fn now(&self) -> SystemTime;
}

pub trait IdSource {
    /// # Errors
    /// Returns an error when a unique valid persisted identifier cannot be produced.
    fn next_recording_id(&mut self, now: SystemTime) -> Result<RecordingId, PortError>;
}
