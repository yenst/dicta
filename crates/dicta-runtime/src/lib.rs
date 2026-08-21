//! Deterministic integration boundary for Dicta's native frontends.

#![forbid(unsafe_code)]

#[cfg(unix)]
pub mod service;

use dicta_capture::CaptureArtifact;
use dicta_control::protocol::{AppPhase, StatusSnapshot, TranscriptionState};
use dicta_control::{
    AnnotationTool, CleanupSummary, Command as ControlCommand, ErrorCode, Event as ControlEvent,
    EventEnvelope, ModelInstallStage, ModelState, ModelStatusSummary, ModelTier, ProjectSummary,
    ProtocolError, RecordingSelector, RecordingSummary, RequestEnvelope, Response,
    ResponseEnvelope, VoiceNoteState, VoiceNoteStatus,
};
use dicta_core::{
    storage::{is_shortcut_id, is_transcription_language, AppSettings},
    AnnotationFile, ProjectFile, ProjectId, RecordingFile, RecordingId, TimelineNote,
    TranscriptionStatus,
};
use dicta_engine::{
    AppSnapshot, AppState, Command as EngineCommand, CommandKind, Controller, ControllerError,
    Operation, RecordingSession, StateKind,
};
use dicta_transcribe::{
    ModelFileState, ModelInstallOutcome, ModelPreparation, ModelPreparationStage, ModelStatus,
    TranscriptionOutput,
};
use std::fmt::Write as _;
use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

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

    fn protocol_error(&self) -> ProtocolError {
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

/// Returns the private per-user directory used only for short-lived voice-note
/// capture files.
///
/// # Errors
/// Returns a permission/security error when the runtime directory is not a
/// private real directory owned by the current user.
#[cfg(unix)]
pub fn voice_note_directory() -> Result<PathBuf, PortError> {
    let socket = dicta_control::socket::default_socket_path().map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not resolve the private Dicta runtime directory: {error}"),
        )
    })?;
    let directory = socket
        .parent()
        .ok_or_else(|| {
            PortError::new(
                PortErrorKind::PermissionDenied,
                "Dicta control socket has no private runtime directory",
            )
        })?
        .join("voice-notes");
    dicta_control::socket::ensure_private_runtime_dir(&directory).map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not prepare private voice-note storage: {error}"),
        )
    })?;
    Ok(directory)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    pub transcribe_after_recording: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            transcribe_after_recording: true,
        }
    }
}

/// Read-only state safe to hand to QML, a CLI server, or an MCP adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeSnapshot {
    pub app: AppSnapshot,
    pub status: StatusSnapshot,
    pub last_event_sequence: u64,
}

/// The correlated response and event frames produced by one request.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlOutput {
    pub response: ResponseEnvelope,
    pub events: Vec<EventEnvelope>,
}

#[derive(Debug)]
pub enum RuntimeError {
    InvalidRequest(String),
    Conflict(ControllerError),
    CommandConflict {
        command: &'static str,
        state: StateKind,
    },
    DataConflict(String),
    Port(PortError),
    EventSequenceExhausted,
}

impl RuntimeError {
    #[must_use]
    pub fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::InvalidRequest(message) => {
                ProtocolError::new(ErrorCode::InvalidRequest, message.clone())
            }
            Self::Conflict(error) => ProtocolError::new(ErrorCode::Conflict, error.to_string()),
            Self::CommandConflict { command, state } => ProtocolError::new(
                ErrorCode::Conflict,
                format!("cannot {command} while application is {state}"),
            ),
            Self::DataConflict(message) => ProtocolError::new(ErrorCode::Conflict, message.clone()),
            Self::Port(error) => error.protocol_error(),
            Self::EventSequenceExhausted => {
                ProtocolError::new(ErrorCode::Internal, "runtime event sequence exhausted")
            }
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::DataConflict(message) => {
                formatter.write_str(message)
            }
            Self::Conflict(error) => error.fmt(formatter),
            Self::CommandConflict { command, state } => {
                write!(formatter, "cannot {command} while application is {state}")
            }
            Self::Port(error) => error.fmt(formatter),
            Self::EventSequenceExhausted => formatter.write_str("runtime event sequence exhausted"),
        }
    }
}

impl Error for RuntimeError {}

impl From<ControllerError> for RuntimeError {
    fn from(error: ControllerError) -> Self {
        Self::Conflict(error)
    }
}

impl From<PortError> for RuntimeError {
    fn from(error: PortError) -> Self {
        Self::Port(error)
    }
}

/// Sole mutable adapter around [`Controller`].
pub struct Runtime<C, T, A, S, K, I> {
    controller: Controller,
    capture: C,
    transcription: T,
    annotations: A,
    storage: S,
    clock: K,
    ids: I,
    config: RuntimeConfig,
    selected_tool: AnnotationTool,
    next_event_sequence: u64,
    events: Vec<ControlEvent>,
    pending_voice_note: Option<PendingVoiceNote>,
    cancelled_voice_inflight: Option<RecordingId>,
    voice_note_status: VoiceNoteStatus,
}

struct PendingVoiceNote {
    recording: RecordingFile,
    note_id: String,
    timestamp_seconds: f64,
    audio_path: PathBuf,
}

impl Drop for PendingVoiceNote {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.audio_path);
    }
}

impl<C, T, A, S, K, I> Runtime<C, T, A, S, K, I>
where
    C: CapturePort,
    T: TranscriptionPort,
    A: AnnotationPort,
    S: StoragePort,
    K: Clock,
    I: IdSource,
{
    #[must_use]
    pub fn new(
        capture: C,
        transcription: T,
        annotations: A,
        storage: S,
        clock: K,
        ids: I,
        config: RuntimeConfig,
    ) -> Self {
        Self {
            controller: Controller::new(),
            capture,
            transcription,
            annotations,
            storage,
            clock,
            ids,
            config,
            selected_tool: AnnotationTool::Pen,
            next_event_sequence: 1,
            events: Vec::new(),
            pending_voice_note: None,
            cancelled_voice_inflight: None,
            voice_note_status: VoiceNoteStatus::default(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            app: self.controller.snapshot(),
            status: self.status(),
            last_event_sequence: self.next_event_sequence.saturating_sub(1),
        }
    }

    #[must_use]
    pub fn events(&self) -> &[ControlEvent] {
        &self.events
    }

    #[must_use]
    pub fn events_since(&self, sequence: Option<u64>) -> Vec<EventEnvelope> {
        let sequence = sequence.unwrap_or(0);
        self.events
            .iter()
            .filter(|event| event_sequence(event) > sequence)
            .cloned()
            .map(EventEnvelope::new)
            .collect()
    }

    /// Polls injected background ports once without blocking the runtime thread.
    ///
    /// A completed worker failure is converted into the normal typed failed
    /// state and is therefore considered consumed. Conflicts and internal event
    /// sequence failures remain errors so stale completions never mutate state.
    ///
    /// # Errors
    /// Returns a conflict for a stale completion or an internal event-sequence
    /// failure.
    pub fn poll_background(&mut self) -> Result<bool, RuntimeError> {
        let mut consumed = false;
        if let Some(completion) = self.transcription.poll_completion() {
            if self
                .pending_voice_note
                .as_ref()
                .is_some_and(|pending| pending.recording.id == completion.recording_id)
            {
                self.complete_voice_note(completion.result)?;
                consumed = true;
            } else if self.cancelled_voice_inflight.as_ref() == Some(&completion.recording_id) {
                self.cancelled_voice_inflight = None;
                self.voice_note_status = VoiceNoteStatus::default();
                consumed = true;
            } else {
                match self.complete_transcription(completion.recording_id, completion.result) {
                    Ok(()) | Err(RuntimeError::Port(_)) => consumed = true,
                    Err(error) => return Err(error),
                }
            }
        }
        if self.transcription.poll_model_install().is_some() {
            consumed = true;
        }
        if self.config.transcribe_after_recording
            && self.transcription.is_available()
            && self.controller.snapshot().state.kind() == StateKind::Idle
            && self.pending_voice_note.is_none()
            && self.cancelled_voice_inflight.is_none()
        {
            if let Some(candidate) = self.storage.poll_transcription_retry() {
                consumed = true;
                if let Ok(recording) = candidate {
                    self.start_retry_transcription(&recording)?;
                }
            }
        }
        Ok(consumed)
    }

    /// Translates one validated wire request into domain work and a stable response.
    pub fn handle(&mut self, request: RequestEnvelope) -> ControlOutput {
        let id = request.id;
        if let Err(error) = request.validate_version() {
            return ControlOutput {
                response: ResponseEnvelope::failure(id, error),
                events: Vec::new(),
            };
        }

        let event_start = self.events.len();
        let queried_events = match &request.command {
            ControlCommand::Events { since_sequence } => Some(*since_sequence),
            _ => None,
        };
        let result = self.apply_control(request.command);
        let events = queried_events.map_or_else(
            || {
                self.events[event_start..]
                    .iter()
                    .cloned()
                    .map(EventEnvelope::new)
                    .collect()
            },
            |since| self.events_since(since),
        );
        let response = match result {
            Ok(response) => ResponseEnvelope::success(id, response),
            Err(error) => ResponseEnvelope::failure(id, error.protocol_error()),
        };
        ControlOutput { response, events }
    }

    /// Completes a pending recorder startup. Stale IDs are rejected atomically.
    ///
    /// # Errors
    /// Returns a conflict for stale completions, the supplied port failure, or an
    /// internal sequence error.
    pub fn complete_capture_start(
        &mut self,
        recording_id: RecordingId,
        result: Result<(), PortError>,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(4)?;
        self.require_recording(
            StateKind::Preparing,
            &recording_id,
            CommandKind::RecordingPrepared,
        )?;
        match result {
            Ok(()) => self.complete_capture_start_inner(recording_id),
            Err(error) => {
                self.raise_failure(Operation::PrepareRecording, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    /// Completes a pending recorder stop. Stale IDs cause no port or storage calls.
    ///
    /// # Errors
    /// Returns a conflict for stale completions, or a typed capture, annotation,
    /// storage, transcription, or sequence failure.
    pub fn complete_capture_stop(
        &mut self,
        recording_id: RecordingId,
        result: Result<CaptureArtifact, PortError>,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(8)?;
        let session = self.require_session(
            StateKind::Stopping,
            &recording_id,
            CommandKind::RecordingStopped,
        )?;
        match result {
            Ok(artifact) => self.complete_capture_stop_inner(&session, &artifact),
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    /// Completes a pending transcription. Stale IDs never reach storage.
    ///
    /// # Errors
    /// Returns a conflict for stale completions, or a typed transcription,
    /// storage, or sequence failure.
    pub fn complete_transcription(
        &mut self,
        recording_id: RecordingId,
        result: Result<TranscriptionOutput, PortError>,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(4)?;
        self.require_recording(
            StateKind::Transcribing,
            &recording_id,
            CommandKind::TranscriptionCompleted,
        )?;
        match result {
            Ok(output) => {
                if let Err(error) = self.storage.save_transcription(&recording_id, &output) {
                    self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                    return Err(error.into());
                }
                let outcome = self
                    .controller
                    .dispatch(EngineCommand::TranscriptionCompleted {
                        recording_id: recording_id.clone(),
                    })?;
                self.publish(ControlEvent::TranscriptionCompleted {
                    sequence: 0,
                    recording_id: recording_id.into_string(),
                })?;
                self.publish_state(&outcome.snapshot)
            }
            Err(error) => {
                self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    /// Reports a recorder failure after startup. Only the active recording matches.
    ///
    /// # Errors
    /// Returns a conflict for stale or invalid failure reports, or a sequence error.
    pub fn capture_failed(
        &mut self,
        recording_id: RecordingId,
        error: PortError,
    ) -> Result<(), RuntimeError> {
        self.ensure_event_capacity(3)?;
        let kind = self.controller.snapshot().state.kind();
        if !matches!(kind, StateKind::Recording | StateKind::Annotating) {
            return Err(ControllerError::UnexpectedOperation {
                operation: Operation::Capture,
                state: kind,
            }
            .into());
        }
        self.require_recording(kind, &recording_id, CommandKind::OperationFailed)?;
        self.raise_failure(Operation::Capture, recording_id, &error)?;
        Err(error.into())
    }

    fn apply_control(&mut self, command: ControlCommand) -> Result<Response, RuntimeError> {
        self.ensure_event_capacity(12)?;
        match command {
            ControlCommand::UiShow => {
                self.publish(ControlEvent::UiShowRequested { sequence: 0 })?;
                Ok(Response::Accepted)
            }
            ControlCommand::Status | ControlCommand::RecordStatus => {
                Ok(Response::Status(self.status()))
            }
            settings_command @ (ControlCommand::SettingsGet
            | ControlCommand::SettingsSetShortcut { .. }
            | ControlCommand::SettingsSetCleanup { .. }
            | ControlCommand::SettingsSetBranchLocking { .. }
            | ControlCommand::SettingsSetLanguage { .. }
            | ControlCommand::SettingsSetGeneralPath { .. }
            | ControlCommand::SettingsCleanupMerged { .. }) => {
                self.apply_settings_control(settings_command)
            }
            ControlCommand::ModelStatus => self.model_status(),
            ControlCommand::ModelInstall { model } => self.install_model(model),
            ControlCommand::Events { .. } => Ok(Response::Accepted),
            ControlCommand::ProjectList => self.list_projects(),
            ControlCommand::ProjectAdd { path, name } => self.add_project(&path, name.as_deref()),
            ControlCommand::ProjectCreate { name } => self.create_project(&name),
            ControlCommand::ProjectRemove { project } => self.remove_project(project),
            ControlCommand::ProjectRefresh { project } => self.refresh_project(project),
            ControlCommand::ProjectSelect { project } => self.select_project(project),
            ControlCommand::ProjectCurrent => self.current_project(),
            ControlCommand::RecordingList {
                project,
                branch,
                limit,
            } => self.list_recordings(project, branch.as_deref(), limit),
            ControlCommand::RecordingShow { recording } => self.show_recording(recording),
            ControlCommand::Context {
                recording, project, ..
            } => self.recording_context(recording, project),
            ControlCommand::RecordingTranscribe { recording } => {
                self.transcribe_existing(recording)
            }
            ControlCommand::RecordingSetTimelineNotes { recording, notes } => {
                self.set_timeline_notes(recording, notes)
            }
            ControlCommand::RecordingVoiceNoteTranscribe {
                recording,
                note_id,
                timestamp_seconds,
                audio_path,
            } => self.transcribe_voice_note(recording, &note_id, timestamp_seconds, &audio_path),
            ControlCommand::RecordingVoiceNoteCancel => Ok(self.cancel_voice_note()),
            ControlCommand::RecordingVoiceNoteStatus => {
                Ok(Response::VoiceNote(self.voice_note_status.clone()))
            }
            ControlCommand::RecordingDelete { recording } => self.delete_recording(recording),
            ControlCommand::RecordStart { project, note } => self.start_recording(project, note),
            ControlCommand::RecordStop => self.stop_recording(),
            ControlCommand::RecordToggle => match self.controller.snapshot().state.kind() {
                StateKind::Idle => self.start_recording(None, None),
                StateKind::Recording | StateKind::Annotating => self.stop_recording(),
                state => Err(ControllerError::InvalidTransition {
                    command: CommandKind::StartRecording,
                    state,
                }
                .into()),
            },
            ControlCommand::AnnotationToggle => {
                if matches!(self.controller.snapshot().state, AppState::Annotating(_)) {
                    self.set_annotations_enabled(false)
                } else {
                    self.enable_pen_annotations()
                }
            }
            ControlCommand::AnnotationEnable => self.enable_pen_annotations(),
            ControlCommand::AnnotationDisable => self.set_annotations_enabled(false),
            ControlCommand::AnnotationTool { tool } => self.set_annotation_tool(tool),
            ControlCommand::AnnotationUndo => self.annotation_edit(AnnotationEdit::Undo),
            ControlCommand::AnnotationClear => self.annotation_edit(AnnotationEdit::Clear),
            ControlCommand::RecordingOpen { recording } => self.open_recording(recording),
        }
    }

    fn apply_settings_control(
        &mut self,
        command: ControlCommand,
    ) -> Result<Response, RuntimeError> {
        match command {
            ControlCommand::SettingsGet => self.settings(),
            ControlCommand::SettingsSetShortcut { shortcut_id } => {
                self.update_settings(|settings| {
                    if !is_shortcut_id(&shortcut_id) {
                        return Err(RuntimeError::InvalidRequest(format!(
                            "unknown shortcut preset `{shortcut_id}`"
                        )));
                    }
                    settings.shortcut_id = shortcut_id;
                    Ok(())
                })
            }
            ControlCommand::SettingsSetCleanup { enabled } => self.update_settings(|settings| {
                settings.cleanup_merged_videos = enabled;
                Ok(())
            }),
            ControlCommand::SettingsSetBranchLocking { enabled } => {
                self.require_idle("change branch locking")?;
                self.update_settings(|settings| {
                    settings.branch_locking = enabled;
                    Ok(())
                })
            }
            ControlCommand::SettingsSetLanguage { language } => {
                if !is_transcription_language(&language) {
                    return Err(RuntimeError::InvalidRequest(format!(
                        "unsupported transcription language `{language}`"
                    )));
                }
                self.transcription.set_language(&language)?;
                self.update_settings(|settings| {
                    settings.transcription_language = language;
                    Ok(())
                })
            }
            ControlCommand::SettingsSetGeneralPath { path } => {
                self.require_idle("change General storage")?;
                let path = validate_general_path(path)?;
                self.update_settings(|settings| {
                    settings.general_path = path;
                    Ok(())
                })
            }
            ControlCommand::SettingsCleanupMerged { project } => {
                self.require_idle("clean merged branch videos")?;
                let project_id = match project {
                    Some(project) => ProjectId::new(project)
                        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                    None => self.controller.snapshot().selected_project.ok_or_else(|| {
                        RuntimeError::InvalidRequest(
                            "select a project or pass one to merged-video cleanup".to_owned(),
                        )
                    })?,
                };
                self.storage
                    .cleanup_merged_videos(&project_id)
                    .map(Response::Cleanup)
                    .map_err(Into::into)
            }
            _ => Err(RuntimeError::InvalidRequest(
                "command is not a settings command".to_owned(),
            )),
        }
    }

    fn settings(&mut self) -> Result<Response, RuntimeError> {
        self.storage
            .load_settings()
            .map(Response::Settings)
            .map_err(Into::into)
    }

    fn update_settings(
        &mut self,
        update: impl FnOnce(&mut AppSettings) -> Result<(), RuntimeError>,
    ) -> Result<Response, RuntimeError> {
        let mut settings = self.storage.load_settings()?;
        update(&mut settings)?;
        let settings = settings.normalized();
        self.storage.save_settings(&settings)?;
        Ok(Response::Settings(settings))
    }

    fn require_idle(&self, command: &'static str) -> Result<(), RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state == StateKind::Idle {
            Ok(())
        } else {
            Err(RuntimeError::CommandConflict { command, state })
        }
    }

    fn model_status(&mut self) -> Result<Response, RuntimeError> {
        self.transcription
            .model_status()
            .map(model_status_summary)
            .map(Response::ModelStatus)
            .map_err(Into::into)
    }

    fn install_model(&mut self, model: ModelTier) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle
            || self.pending_voice_note.is_some()
            || self.cancelled_voice_inflight.is_some()
        {
            return Err(RuntimeError::CommandConflict {
                command: "install a transcription model",
                state,
            });
        }
        match model {
            ModelTier::Quality => match self.transcription.install_quality_model()? {
                Completion::Ready(outcome) => {
                    Ok(Response::ModelStatus(model_status_summary(outcome.status)))
                }
                Completion::Pending => Ok(Response::ModelInstallStarted),
            },
        }
    }

    fn list_projects(&mut self) -> Result<Response, RuntimeError> {
        let selected = self.controller.snapshot().selected_project;
        let mut projects = self.storage.load_projects()?;
        projects.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(Response::Projects(
            projects
                .iter()
                .map(|project| project_summary(project, selected.as_ref()))
                .collect(),
        ))
    }

    fn add_project(&mut self, path: &str, name: Option<&str>) -> Result<Response, RuntimeError> {
        self.storage.add_project(path, name)?;
        Ok(Response::Accepted)
    }

    fn create_project(&mut self, name: &str) -> Result<Response, RuntimeError> {
        self.require_idle_project_mutation("create a project")?;
        let project = self.storage.create_project(name)?;
        self.select_project(project.id.into_string())
    }

    fn remove_project(&mut self, project: String) -> Result<Response, RuntimeError> {
        self.require_idle_project_mutation("remove a project")?;
        let project_id = ProjectId::new(project)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        if project_id.as_str() == dicta_core::GENERAL_PROJECT_ID {
            return Err(RuntimeError::InvalidRequest(
                "General cannot be removed".to_owned(),
            ));
        }
        self.storage.remove_project(&project_id)?;
        if self.controller.snapshot().selected_project.as_ref() == Some(&project_id) {
            let outcome = self
                .controller
                .dispatch(EngineCommand::SelectProject(None))?;
            self.publish_state(&outcome.snapshot)?;
        }
        Ok(Response::Accepted)
    }

    fn refresh_project(&mut self, project: String) -> Result<Response, RuntimeError> {
        let project_id = ProjectId::new(project)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let selected = self.controller.snapshot().selected_project;
        let project = self
            .storage
            .load_projects()?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("project {project_id} was not found"),
                )
            })?;
        Ok(Response::Project(Some(project_summary(
            &project,
            selected.as_ref(),
        ))))
    }

    fn require_idle_project_mutation(&self, command: &'static str) -> Result<(), RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state == StateKind::Idle {
            Ok(())
        } else {
            Err(RuntimeError::CommandConflict { command, state })
        }
    }

    fn select_project(&mut self, project: String) -> Result<Response, RuntimeError> {
        let project_id = ProjectId::new(project)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let projects = self.storage.load_projects()?;
        if !projects.iter().any(|project| project.id == project_id) {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                format!("project {project_id} was not found"),
            )
            .into());
        }
        let outcome = self
            .controller
            .dispatch(EngineCommand::SelectProject(Some(project_id)))?;
        self.publish_state(&outcome.snapshot)?;
        Ok(Response::Accepted)
    }

    fn current_project(&mut self) -> Result<Response, RuntimeError> {
        let Some(selected) = self.controller.snapshot().selected_project else {
            return Ok(Response::Project(None));
        };
        let project = self
            .storage
            .load_projects()?
            .into_iter()
            .find(|project| project.id == selected)
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("selected project {selected} was not found"),
                )
            })?;
        Ok(Response::Project(Some(project_summary(
            &project,
            Some(&selected),
        ))))
    }

    fn list_recordings(
        &mut self,
        project: Option<String>,
        branch: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Response, RuntimeError> {
        let project = project
            .map(|value| {
                ProjectId::new(value)
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let mut recordings = self.load_recordings_checked()?;
        recordings.retain(|recording| {
            project
                .as_ref()
                .is_none_or(|project| &recording.project_id == project)
                && branch.is_none_or(|branch| recording.git_branch.as_deref() == Some(branch))
        });
        sort_recordings_latest_first(&mut recordings);
        if let Some(limit) = limit {
            recordings.truncate(limit as usize);
        }
        Ok(Response::Recordings(
            recordings.iter().map(recording_summary).collect(),
        ))
    }

    fn show_recording(&mut self, selector: RecordingSelector) -> Result<Response, RuntimeError> {
        self.resolve_recording(selector)
            .map(Box::new)
            .map(Response::RecordingDetails)
    }

    fn open_recording(&mut self, selector: RecordingSelector) -> Result<Response, RuntimeError> {
        let recording = self.resolve_recording(selector)?;
        self.publish(ControlEvent::UiRecordingRequested {
            sequence: 0,
            recording_id: recording.id.into_string(),
        })?;
        Ok(Response::Accepted)
    }

    fn recording_context(
        &mut self,
        selector: RecordingSelector,
        project: Option<String>,
    ) -> Result<Response, RuntimeError> {
        let project_filter = project
            .map(|value| {
                ProjectId::new(value)
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let mut recordings = self.load_recordings_checked()?;
        if let Some(project) = project_filter.as_ref() {
            recordings.retain(|recording| &recording.project_id == project);
        }
        let recording = resolve_recording_from(recordings, selector)?;
        let project_name = self
            .storage
            .load_projects()?
            .into_iter()
            .find(|project| project.id == recording.project_id)
            .map_or_else(|| recording.project_id.to_string(), |project| project.name);
        Ok(Response::Context {
            text: render_recording_context(&recording, &project_name),
        })
    }

    fn transcribe_existing(
        &mut self,
        selector: RecordingSelector,
    ) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle
            || self.pending_voice_note.is_some()
            || self.cancelled_voice_inflight.is_some()
        {
            return Err(ControllerError::InvalidTransition {
                command: CommandKind::TranscribeRecording,
                state,
            }
            .into());
        }
        let recording = self.resolve_recording(selector)?;
        let recording_id = recording.id.clone();
        let outcome = self
            .controller
            .dispatch(EngineCommand::TranscribeRecording {
                recording_id: recording_id.clone(),
            })?;
        self.publish_state(&outcome.snapshot)?;
        self.storage.mark_transcription_pending(&recording_id)?;
        match self.transcription.transcribe(&recording) {
            Ok(Completion::Ready(output)) => {
                self.complete_transcription(recording_id, Ok(output))?;
            }
            Ok(Completion::Pending) => {}
            Err(error) => {
                self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                return Err(error.into());
            }
        }
        Ok(Response::Accepted)
    }

    fn set_timeline_notes(
        &mut self,
        selector: RecordingSelector,
        mut notes: Vec<TimelineNote>,
    ) -> Result<Response, RuntimeError> {
        if self.pending_voice_note.is_some() || self.cancelled_voice_inflight.is_some() {
            return Err(RuntimeError::InvalidRequest(
                "timeline notes cannot change while a voice note is processing".to_owned(),
            ));
        }
        let recording = self.resolve_recording(selector)?;
        validate_timeline_notes(&recording, &notes)?;
        notes.sort_by(|left, right| {
            left.timestamp_seconds
                .total_cmp(&right.timestamp_seconds)
                .then_with(|| left.id.cmp(&right.id))
        });
        self.storage
            .save_timeline_notes(&recording, &notes)
            .map(Box::new)
            .map(Response::RecordingDetails)
            .map_err(Into::into)
    }

    fn transcribe_voice_note(
        &mut self,
        selector: RecordingSelector,
        note_id: &str,
        timestamp_seconds: f64,
        audio_path: &str,
    ) -> Result<Response, RuntimeError> {
        self.require_idle("transcribe a voice note")?;
        if self.pending_voice_note.is_some() || self.cancelled_voice_inflight.is_some() {
            return Err(RuntimeError::InvalidRequest(
                "a voice-note job is already active".to_owned(),
            ));
        }
        if !self.transcription.is_available() {
            return Err(PortError::new(
                PortErrorKind::Unavailable,
                "local transcription is unavailable for voice notes",
            )
            .into());
        }
        let note_id = note_id.trim();
        if note_id.is_empty()
            || note_id.len() > 128
            || note_id.chars().any(char::is_control)
            || !timestamp_seconds.is_finite()
            || timestamp_seconds < 0.0
        {
            return Err(RuntimeError::InvalidRequest(
                "voice-note identity or timestamp is invalid".to_owned(),
            ));
        }
        let recording = self.resolve_recording(selector)?;
        if recording
            .duration_seconds
            .is_some_and(|duration| timestamp_seconds > duration + 1.0)
        {
            return Err(RuntimeError::InvalidRequest(
                "voice-note timestamp is outside the recording".to_owned(),
            ));
        }
        let audio_path = validate_voice_note_audio(Path::new(audio_path))?;
        let mut input = recording.clone();
        input.video_path = audio_path.to_string_lossy().into_owned();
        let pending = PendingVoiceNote {
            recording,
            note_id: note_id.to_owned(),
            timestamp_seconds,
            audio_path,
        };
        self.voice_note_status = VoiceNoteStatus {
            state: VoiceNoteState::Processing,
            recording_id: Some(pending.recording.id.to_string()),
            note_id: Some(pending.note_id.clone()),
            message: "Transcribing voice note…".to_owned(),
        };
        match self.transcription.transcribe(&input) {
            Ok(Completion::Ready(output)) => {
                self.pending_voice_note = Some(pending);
                self.complete_voice_note(Ok(output))?;
            }
            Ok(Completion::Pending) => self.pending_voice_note = Some(pending),
            Err(error) => {
                self.voice_note_status.state = VoiceNoteState::Failed;
                self.voice_note_status.message.clone_from(&error.message);
                drop(pending);
                return Err(error.into());
            }
        }
        Ok(Response::VoiceNote(self.voice_note_status.clone()))
    }

    fn complete_voice_note(
        &mut self,
        result: Result<TranscriptionOutput, PortError>,
    ) -> Result<(), RuntimeError> {
        let Some(pending) = self.pending_voice_note.take() else {
            return Err(RuntimeError::InvalidRequest(
                "voice-note completion is stale".to_owned(),
            ));
        };
        match result {
            Ok(output) if !output.transcript.trim().is_empty() => {
                let mut notes = pending.recording.timeline_notes.clone();
                notes.push(TimelineNote::voice(
                    pending.note_id.clone(),
                    pending.timestamp_seconds,
                    output.transcript.trim(),
                    self.clock.now(),
                ));
                validate_timeline_notes(&pending.recording, &notes)?;
                notes.sort_by(|left, right| {
                    left.timestamp_seconds
                        .total_cmp(&right.timestamp_seconds)
                        .then_with(|| left.id.cmp(&right.id))
                });
                self.storage
                    .save_timeline_notes(&pending.recording, &notes)?;
                self.voice_note_status = VoiceNoteStatus {
                    state: VoiceNoteState::Complete,
                    recording_id: Some(pending.recording.id.to_string()),
                    note_id: Some(pending.note_id.clone()),
                    message: "Voice note added.".to_owned(),
                };
                Ok(())
            }
            Ok(_) => {
                self.voice_note_status.state = VoiceNoteState::Failed;
                "Voice-note transcription was empty."
                    .clone_into(&mut self.voice_note_status.message);
                Ok(())
            }
            Err(error) => {
                self.voice_note_status.state = VoiceNoteState::Failed;
                self.voice_note_status.message.clone_from(&error.message);
                Ok(())
            }
        }
    }

    fn cancel_voice_note(&mut self) -> Response {
        let Some(pending) = self.pending_voice_note.take() else {
            return Response::VoiceNote(self.voice_note_status.clone());
        };
        self.cancelled_voice_inflight = Some(pending.recording.id.clone());
        self.voice_note_status = VoiceNoteStatus {
            state: VoiceNoteState::Cancelling,
            recording_id: Some(pending.recording.id.to_string()),
            note_id: Some(pending.note_id.clone()),
            message: "Cancelling voice note…".to_owned(),
        };
        drop(pending);
        Response::VoiceNote(self.voice_note_status.clone())
    }

    fn start_retry_transcription(&mut self, recording: &RecordingFile) -> Result<(), RuntimeError> {
        if !recording.is_valid() {
            return Err(RuntimeError::InvalidRequest(
                "retry discovery returned invalid recording metadata".to_owned(),
            ));
        }
        let recording_id = recording.id.clone();
        let outcome = self
            .controller
            .dispatch(EngineCommand::TranscribeRecording {
                recording_id: recording_id.clone(),
            })?;
        self.publish_state(&outcome.snapshot)?;
        self.storage.mark_transcription_pending(&recording_id)?;
        match self.transcription.transcribe(recording) {
            Ok(Completion::Ready(output)) => self.complete_transcription(recording_id, Ok(output)),
            Ok(Completion::Pending) => Ok(()),
            Err(error) => {
                self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    fn delete_recording(&mut self, selector: RecordingSelector) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle {
            return Err(RuntimeError::CommandConflict {
                command: "delete a recording",
                state,
            });
        }
        let recording = self.resolve_recording(selector)?;
        self.storage.delete_recording(&recording)?;
        Ok(Response::Accepted)
    }

    fn resolve_recording(
        &mut self,
        selector: RecordingSelector,
    ) -> Result<RecordingFile, RuntimeError> {
        resolve_recording_from(self.load_recordings_checked()?, selector)
    }

    fn load_recordings_checked(&mut self) -> Result<Vec<RecordingFile>, RuntimeError> {
        let recordings = self.storage.load_recordings()?;
        if recordings.iter().any(|recording| !recording.is_valid()) {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "recording catalog returned invalid metadata",
            )
            .into());
        }
        Ok(recordings)
    }

    fn start_recording(
        &mut self,
        project: Option<String>,
        note: Option<String>,
    ) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle
            || self.pending_voice_note.is_some()
            || self.cancelled_voice_inflight.is_some()
        {
            return Err(ControllerError::InvalidTransition {
                command: CommandKind::StartRecording,
                state,
            }
            .into());
        }
        let project_id = project
            .map(|value| {
                ProjectId::new(value)
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let recording_id = self.ids.next_recording_id(self.clock.now())?;
        if let Some(project_id) = project_id {
            let outcome = self
                .controller
                .dispatch(EngineCommand::SelectProject(Some(project_id)))?;
            self.publish_state(&outcome.snapshot)?;
        }
        let outcome = self.controller.dispatch(EngineCommand::StartRecording {
            recording_id: recording_id.clone(),
            note,
        })?;
        self.publish_state(&outcome.snapshot)?;
        let session = session_from_state(&outcome.snapshot.state).clone();
        match self.capture.start(&session) {
            Ok(Completion::Ready(())) => self.complete_capture_start_inner(recording_id)?,
            Ok(Completion::Pending) => {}
            Err(error) => {
                self.raise_failure(Operation::PrepareRecording, recording_id, &error)?;
                return Err(error.into());
            }
        }
        Ok(Response::Accepted)
    }

    fn complete_capture_start_inner(
        &mut self,
        recording_id: RecordingId,
    ) -> Result<(), RuntimeError> {
        let outcome = self.controller.dispatch(EngineCommand::RecordingPrepared {
            recording_id: recording_id.clone(),
        })?;
        self.publish(ControlEvent::RecordingStarted {
            sequence: 0,
            recording_id: recording_id.into_string(),
        })?;
        self.publish_state(&outcome.snapshot)
    }

    fn stop_recording(&mut self) -> Result<Response, RuntimeError> {
        let outcome = self.controller.dispatch(EngineCommand::StopRecording)?;
        self.publish_state(&outcome.snapshot)?;
        let session = session_from_state(&outcome.snapshot.state).clone();
        let recording_id = session.recording_id.clone();
        match self.capture.stop(&session) {
            Ok(Completion::Ready(artifact)) => {
                self.complete_capture_stop_inner(&session, &artifact)?;
            }
            Ok(Completion::Pending) => {}
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                return Err(error.into());
            }
        }
        Ok(Response::Accepted)
    }

    fn complete_capture_stop_inner(
        &mut self,
        session: &RecordingSession,
        artifact: &CaptureArtifact,
    ) -> Result<(), RuntimeError> {
        let recording_id = session.recording_id.clone();
        if artifact.path.as_os_str().is_empty()
            || artifact.output_name.trim().is_empty()
            || artifact.geometry.width == 0
            || artifact.geometry.height == 0
            || artifact.scale_milli == 0
            || artifact.encoded_pixel_size.0 == 0
            || artifact.encoded_pixel_size.1 == 0
        {
            let error = PortError::new(
                PortErrorKind::Internal,
                "capture port returned an invalid artifact",
            );
            self.raise_failure(Operation::StopRecording, recording_id, &error)?;
            return Err(error.into());
        }
        let annotations = match self.annotations.finish(&recording_id) {
            Ok(annotations) => annotations,
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                return Err(error.into());
            }
        };
        if annotations
            .as_ref()
            .is_some_and(|document| !document.is_valid() || document.recording_id != recording_id)
        {
            let error = PortError::new(
                PortErrorKind::Internal,
                "annotation port returned an invalid document",
            );
            self.raise_failure(Operation::StopRecording, recording_id, &error)?;
            return Err(error.into());
        }
        let saved = match self
            .storage
            .save_recording(session, artifact, annotations.as_ref())
        {
            Ok(recording) => recording,
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                return Err(error.into());
            }
        };
        if !saved.is_valid() || saved.id != recording_id {
            let error = PortError::new(
                PortErrorKind::Internal,
                "storage returned invalid recording metadata",
            );
            self.raise_failure(Operation::StopRecording, recording_id, &error)?;
            return Err(error.into());
        }
        let duration_seconds = artifact.duration.as_secs_f64();
        let should_transcribe =
            self.config.transcribe_after_recording && self.transcription.is_available();
        let outcome = self.controller.dispatch(EngineCommand::RecordingStopped {
            recording_id: recording_id.clone(),
            transcribe: should_transcribe,
        })?;
        self.publish(ControlEvent::RecordingStopped {
            sequence: 0,
            recording_id: recording_id.clone().into_string(),
            duration_seconds,
        })?;
        self.publish_state(&outcome.snapshot)?;
        if should_transcribe {
            self.storage.mark_transcription_pending(&recording_id)?;
            match self.transcription.transcribe(&saved) {
                Ok(Completion::Ready(output)) => {
                    self.complete_transcription(recording_id, Ok(output))?;
                }
                Ok(Completion::Pending) => {}
                Err(error) => {
                    self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    fn set_annotations_enabled(&mut self, enabled: bool) -> Result<Response, RuntimeError> {
        let command = if enabled {
            EngineCommand::StartAnnotating
        } else {
            EngineCommand::StopAnnotating
        };
        let outcome = self.controller.dispatch(command)?;
        let session = session_from_state(&outcome.snapshot.state).clone();
        if let Err(error) = self.annotations.set_enabled(&session.recording_id, enabled) {
            self.raise_failure(Operation::Capture, session.recording_id, &error)?;
            return Err(error.into());
        }
        self.publish_state(&outcome.snapshot)?;
        Ok(Response::Accepted)
    }

    fn enable_pen_annotations(&mut self) -> Result<Response, RuntimeError> {
        self.set_annotations_enabled(true)?;
        self.set_annotation_tool(AnnotationTool::Pen)
    }

    fn set_annotation_tool(&mut self, tool: AnnotationTool) -> Result<Response, RuntimeError> {
        let session = self.require_annotating_session()?;
        if let Err(error) = self.annotations.set_tool(&session.recording_id, tool) {
            self.raise_failure(Operation::Capture, session.recording_id, &error)?;
            return Err(error.into());
        }
        self.selected_tool = tool;
        self.publish_current_state()?;
        Ok(Response::Accepted)
    }

    fn annotation_edit(&mut self, edit: AnnotationEdit) -> Result<Response, RuntimeError> {
        let session = self.require_annotating_session()?;
        let result = match edit {
            AnnotationEdit::Undo => self.annotations.undo(&session.recording_id),
            AnnotationEdit::Clear => self.annotations.clear(&session.recording_id),
        };
        if let Err(error) = result {
            self.raise_failure(Operation::Capture, session.recording_id, &error)?;
            return Err(error.into());
        }
        Ok(Response::Accepted)
    }

    fn require_annotating_session(&self) -> Result<RecordingSession, RuntimeError> {
        let snapshot = self.controller.snapshot();
        if snapshot.state.kind() != StateKind::Annotating {
            return Err(RuntimeError::CommandConflict {
                command: "edit annotations",
                state: snapshot.state.kind(),
            });
        }
        Ok(session_from_state(&snapshot.state).clone())
    }

    fn require_session(
        &self,
        state: StateKind,
        recording_id: &RecordingId,
        command: CommandKind,
    ) -> Result<RecordingSession, RuntimeError> {
        self.require_recording(state, recording_id, command)?;
        Ok(session_from_state(&self.controller.snapshot().state).clone())
    }

    fn require_recording(
        &self,
        state: StateKind,
        recording_id: &RecordingId,
        command: CommandKind,
    ) -> Result<(), RuntimeError> {
        let snapshot = self.controller.snapshot();
        if snapshot.state.kind() != state {
            return Err(ControllerError::InvalidTransition {
                command,
                state: snapshot.state.kind(),
            }
            .into());
        }
        let current_id = recording_id_from_state(&snapshot.state);
        if let Some(expected) = current_id {
            if expected != recording_id {
                return Err(ControllerError::WrongRecording {
                    command,
                    expected: expected.clone(),
                    received: recording_id.clone(),
                }
                .into());
            }
        }
        Ok(())
    }

    fn raise_failure(
        &mut self,
        operation: Operation,
        recording_id: RecordingId,
        error: &PortError,
    ) -> Result<(), RuntimeError> {
        if matches!(operation, Operation::Transcribe) {
            let _ = self
                .storage
                .mark_transcription_failed(&recording_id, &error.message);
        }
        let message = if error.message.trim().is_empty() {
            "operation failed".to_owned()
        } else {
            error.message.clone()
        };
        let outcome = self.controller.dispatch(EngineCommand::OperationFailed {
            operation,
            recording_id,
            message,
        })?;
        self.publish(ControlEvent::Failed {
            sequence: 0,
            error: error.protocol_error(),
        })?;
        self.publish_state(&outcome.snapshot)
    }

    fn status(&self) -> StatusSnapshot {
        status_from_snapshot(&self.controller.snapshot(), self.selected_tool)
    }

    fn publish_current_state(&mut self) -> Result<(), RuntimeError> {
        let snapshot = self.controller.snapshot();
        self.publish_state(&snapshot)
    }

    fn publish_state(&mut self, snapshot: &AppSnapshot) -> Result<(), RuntimeError> {
        self.publish(ControlEvent::StateChanged {
            sequence: 0,
            status: status_from_snapshot(snapshot, self.selected_tool),
        })
    }

    fn publish(&mut self, event: ControlEvent) -> Result<(), RuntimeError> {
        let sequence = self.next_event_sequence;
        self.next_event_sequence = sequence
            .checked_add(1)
            .ok_or(RuntimeError::EventSequenceExhausted)?;
        self.events.push(with_sequence(event, sequence));
        Ok(())
    }

    fn ensure_event_capacity(&self, count: u64) -> Result<(), RuntimeError> {
        self.next_event_sequence
            .checked_add(count)
            .ok_or(RuntimeError::EventSequenceExhausted)
            .map(|_| ())
    }
}

#[derive(Clone, Copy)]
enum AnnotationEdit {
    Undo,
    Clear,
}

fn session_from_state(state: &AppState) -> &RecordingSession {
    match state {
        AppState::Preparing(session)
        | AppState::Recording(session)
        | AppState::Annotating(session)
        | AppState::Stopping(session) => session,
        AppState::Idle | AppState::Transcribing { .. } | AppState::Failed(_) => {
            unreachable!("caller validated a recording session state")
        }
    }
}

fn recording_id_from_state(state: &AppState) -> Option<&RecordingId> {
    match state {
        AppState::Preparing(session)
        | AppState::Recording(session)
        | AppState::Annotating(session)
        | AppState::Stopping(session) => Some(&session.recording_id),
        AppState::Transcribing { recording_id } => Some(recording_id),
        AppState::Failed(failure) => Some(&failure.recording_id),
        AppState::Idle => None,
    }
}

#[cfg(unix)]
fn validate_voice_note_audio(path: &Path) -> Result<PathBuf, PortError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if !path.is_absolute() || path.extension().and_then(|value| value.to_str()) != Some("wav") {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "voice-note audio must be an absolute WAV path",
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        PortError::new(
            PortErrorKind::NotFound,
            format!("could not inspect voice-note audio: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != dicta_control::socket::effective_user_id()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() <= 44
        || metadata.len() > 64 * 1024 * 1024
    {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "voice-note audio is not a private, bounded regular WAV file",
        ));
    }
    let expected = voice_note_directory()?.canonicalize().map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not resolve private voice-note storage: {error}"),
        )
    })?;
    let canonical = path.canonicalize().map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("could not resolve voice-note audio: {error}"),
        )
    })?;
    if canonical.parent() != Some(expected.as_path()) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "voice-note audio escaped private runtime storage",
        ));
    }
    Ok(canonical)
}

#[cfg(not(unix))]
fn validate_voice_note_audio(_path: &Path) -> Result<PathBuf, PortError> {
    Err(PortError::new(
        PortErrorKind::Unavailable,
        "voice notes require the native Linux runtime",
    ))
}

fn status_from_snapshot(snapshot: &AppSnapshot, tool: AnnotationTool) -> StatusSnapshot {
    let annotations_enabled = matches!(snapshot.state, AppState::Annotating(_));
    let phase = match snapshot.state {
        AppState::Idle => AppPhase::Idle,
        AppState::Preparing(_) => AppPhase::Preparing,
        AppState::Recording(_) | AppState::Annotating(_) => AppPhase::Recording,
        AppState::Stopping(_) => AppPhase::Stopping,
        AppState::Transcribing { .. } => AppPhase::Transcribing,
        AppState::Failed(_) => AppPhase::Failed,
    };
    StatusSnapshot {
        phase,
        project: snapshot.selected_project.as_ref().map(ToString::to_string),
        recording_id: recording_id_from_state(&snapshot.state).map(ToString::to_string),
        annotations_enabled,
        annotation_tool: annotations_enabled.then_some(tool),
    }
}

fn project_summary(project: &ProjectFile, selected: Option<&ProjectId>) -> ProjectSummary {
    let branch = project
        .source_path
        .as_deref()
        .and_then(|path| dicta_core::git::branch(Path::new(path)).ok());
    ProjectSummary {
        id: project.id.to_string(),
        name: project.name.clone(),
        path: project.source_path.clone(),
        branch,
        selected: selected == Some(&project.id),
    }
}

fn model_status_summary(status: ModelStatus) -> ModelStatusSummary {
    let install_stage = status
        .install_progress
        .as_ref()
        .map(|progress| match progress.stage {
            ModelPreparationStage::Locating => ModelInstallStage::Locating,
            ModelPreparationStage::Downloading => ModelInstallStage::Downloading,
            ModelPreparationStage::Verifying => ModelInstallStage::Verifying,
            ModelPreparationStage::Ready => ModelInstallStage::Ready,
        });
    let quality_state = if install_stage.is_some_and(|stage| stage != ModelInstallStage::Ready) {
        ModelState::Installing
    } else {
        match status.quality.state {
            ModelFileState::Missing => ModelState::Missing,
            ModelFileState::Partial => ModelState::Partial,
            ModelFileState::Ready => ModelState::Ready,
            ModelFileState::Invalid => ModelState::Invalid,
            ModelFileState::Unverified => ModelState::Unverified,
        }
    };
    let downloaded_bytes = status
        .install_progress
        .as_ref()
        .map(|progress| progress.completed_bytes);
    let message = status.install_progress.as_ref().map_or_else(
        || status.quality.detail.clone(),
        |progress| progress.message.clone(),
    );
    ModelStatusSummary {
        active_model: status
            .active_model
            .as_ref()
            .map(|model| model.kind.label().to_owned()),
        active_model_path: status
            .active_model
            .map(|model| model.path.to_string_lossy().into_owned()),
        quality_state,
        quality_path: status.quality.path.to_string_lossy().into_owned(),
        quality_size_bytes: status.quality.size_bytes,
        expected_download_bytes: status.quality.expected_download_bytes,
        install_stage,
        downloaded_bytes,
        message,
        last_error: status.install_error,
    }
}

fn recording_summary(recording: &RecordingFile) -> RecordingSummary {
    RecordingSummary {
        id: recording.id.to_string(),
        project: Some(recording.project_id.to_string()),
        branch: recording.git_branch.clone(),
        started_at: recording.started_at.map(|value| value.to_rfc3339()),
        note: recording.note.clone(),
        transcript_preview: recording
            .transcript
            .as_deref()
            .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(180).collect()),
        success: recording.success,
        recording_scope: recording.recording_scope.to_string(),
        timeline_note_count: u32::try_from(recording.timeline_notes.len()).unwrap_or(u32::MAX),
        has_annotations: recording.annotation_path.is_some(),
        duration_seconds: recording.duration_seconds.unwrap_or(0.0),
        transcription: match recording.transcription_status {
            TranscriptionStatus::Pending => TranscriptionState::Pending,
            TranscriptionStatus::Processing => TranscriptionState::Processing,
            TranscriptionStatus::Complete => TranscriptionState::Complete,
            TranscriptionStatus::Failed => TranscriptionState::Failed,
            TranscriptionStatus::Unknown => TranscriptionState::Unavailable,
        },
    }
}

fn resolve_recording_from(
    mut recordings: Vec<RecordingFile>,
    selector: RecordingSelector,
) -> Result<RecordingFile, RuntimeError> {
    match selector {
        RecordingSelector::Latest => {
            sort_recordings_latest_first(&mut recordings);
            recordings.into_iter().next().ok_or_else(|| {
                PortError::new(PortErrorKind::NotFound, "no recordings were found").into()
            })
        }
        RecordingSelector::Id(value) => {
            let recording_id = RecordingId::new(value)
                .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
            let mut matches = recordings
                .into_iter()
                .filter(|recording| recording.id == recording_id);
            let recording = matches.next().ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("recording {recording_id} was not found"),
                )
            })?;
            if matches.next().is_some() {
                return Err(RuntimeError::DataConflict(format!(
                    "recording ID {recording_id} exists in more than one project"
                )));
            }
            Ok(recording)
        }
    }
}

const CONTEXT_TRANSCRIPT_LIMIT: usize = 1_200;

fn transcript_excerpt(transcript: &str) -> String {
    let transcript = transcript.split_whitespace().collect::<Vec<_>>().join(" ");
    if transcript.chars().count() <= CONTEXT_TRANSCRIPT_LIMIT {
        return transcript;
    }
    let mut excerpt = transcript
        .chars()
        .take(CONTEXT_TRANSCRIPT_LIMIT)
        .collect::<String>();
    if let Some(boundary) = excerpt.rfind(char::is_whitespace) {
        excerpt.truncate(boundary);
    }
    excerpt.push_str("…\n\n_(Transcript truncated; open the recording for the full text.)_");
    excerpt
}

fn render_recording_context(recording: &RecordingFile, project_name: &str) -> String {
    let mut output = format!(
        "# Dicta recording: {}\n\nProject: {} (`{}`)\n",
        recording.id, project_name, recording.project_id
    );
    if let Some(branch) = recording.git_branch.as_deref() {
        let _ = writeln!(output, "Branch: `{branch}`");
    }
    let _ = writeln!(output, "Scope: {}", recording.recording_scope);
    if !recording.note.trim().is_empty() {
        let _ = writeln!(output, "\n## Note\n\n{}", recording.note.trim());
    }
    if let Some(transcript) = recording
        .transcript
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let excerpt = transcript_excerpt(transcript);
        let _ = writeln!(output, "\n## Transcript excerpt\n\n{excerpt}");
    }
    if !recording.timeline_notes.is_empty() {
        output.push_str("\n## Timeline notes\n");
        for note in &recording.timeline_notes {
            let total_seconds =
                std::time::Duration::try_from_secs_f64(note.timestamp_seconds.max(0.0))
                    .map_or(0, |duration| duration.as_secs());
            let _ = write!(
                output,
                "\n- [{:02}:{:02}] {}",
                total_seconds / 60,
                total_seconds % 60,
                note.text.trim()
            );
        }
        output.push('\n');
    }
    output
}

fn validate_general_path(path: Option<String>) -> Result<Option<String>, RuntimeError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    if path.chars().count() > 4096 || path.chars().any(char::is_control) {
        return Err(RuntimeError::InvalidRequest(
            "General storage path is too long or contains control characters".to_owned(),
        ));
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(RuntimeError::InvalidRequest(
            "General storage path must be absolute".to_owned(),
        ));
    }
    Ok(Some(path.to_owned()))
}

fn validate_timeline_notes(
    recording: &RecordingFile,
    notes: &[TimelineNote],
) -> Result<(), RuntimeError> {
    if notes.len() > 500 {
        return Err(RuntimeError::InvalidRequest(
            "a recording can contain at most 500 timeline notes".to_owned(),
        ));
    }
    let duration = recording.duration_seconds;
    let mut ids = std::collections::HashSet::with_capacity(notes.len());
    for note in notes {
        if !note.is_valid()
            || note.text.chars().count() > 2_000
            || !matches!(note.source.as_str(), "typed" | "voice")
            || !ids.insert(note.id.as_str())
        {
            return Err(RuntimeError::InvalidRequest(
                "one or more timeline notes are invalid".to_owned(),
            ));
        }
        if duration.is_some_and(|duration| note.timestamp_seconds > duration + 0.5) {
            return Err(RuntimeError::InvalidRequest(
                "a timeline note cannot be placed beyond the end of the recording".to_owned(),
            ));
        }
    }
    Ok(())
}

fn sort_recordings_latest_first(recordings: &mut [RecordingFile]) {
    recordings.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.id.cmp(&left.id))
            .then_with(|| right.project_id.cmp(&left.project_id))
    });
}

fn event_sequence(event: &ControlEvent) -> u64 {
    match event {
        ControlEvent::UiShowRequested { sequence }
        | ControlEvent::UiRecordingRequested { sequence, .. }
        | ControlEvent::StateChanged { sequence, .. }
        | ControlEvent::RecordingStarted { sequence, .. }
        | ControlEvent::RecordingStopped { sequence, .. }
        | ControlEvent::AnnotationCreated { sequence, .. }
        | ControlEvent::TranscriptionCompleted { sequence, .. }
        | ControlEvent::Failed { sequence, .. } => *sequence,
    }
}

fn with_sequence(event: ControlEvent, value: u64) -> ControlEvent {
    match event {
        ControlEvent::UiShowRequested { .. } => ControlEvent::UiShowRequested { sequence: value },
        ControlEvent::UiRecordingRequested { recording_id, .. } => {
            ControlEvent::UiRecordingRequested {
                sequence: value,
                recording_id,
            }
        }
        ControlEvent::StateChanged { status, .. } => ControlEvent::StateChanged {
            sequence: value,
            status,
        },
        ControlEvent::RecordingStarted { recording_id, .. } => ControlEvent::RecordingStarted {
            sequence: value,
            recording_id,
        },
        ControlEvent::RecordingStopped {
            recording_id,
            duration_seconds,
            ..
        } => ControlEvent::RecordingStopped {
            sequence: value,
            recording_id,
            duration_seconds,
        },
        ControlEvent::AnnotationCreated {
            tool,
            timestamp_seconds,
            ..
        } => ControlEvent::AnnotationCreated {
            sequence: value,
            tool,
            timestamp_seconds,
        },
        ControlEvent::TranscriptionCompleted { recording_id, .. } => {
            ControlEvent::TranscriptionCompleted {
                sequence: value,
                recording_id,
            }
        }
        ControlEvent::Failed { error, .. } => ControlEvent::Failed {
            sequence: value,
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use dicta_capture::{CaptureBackend, Geometry};
    use dicta_control::{protocol::ResponsePayload, RequestId, PROTOCOL_VERSION};
    use dicta_core::{RecordingScope, TranscriptSegment, TranscriptionStatus};
    use std::{
        collections::VecDeque,
        num::NonZeroU64,
        path::PathBuf,
        time::{Duration, UNIX_EPOCH},
    };

    #[derive(Clone, Copy)]
    enum StartMode {
        Ready,
        Pending,
        Fail,
    }

    #[derive(Clone, Copy)]
    enum StopMode {
        Ready,
        Pending,
    }

    struct FakeCapture {
        start_mode: StartMode,
        stop_mode: StopMode,
        starts: usize,
        stops: usize,
    }

    impl CapturePort for FakeCapture {
        fn start(&mut self, _session: &RecordingSession) -> Result<Completion<()>, PortError> {
            self.starts += 1;
            match self.start_mode {
                StartMode::Ready => Ok(Completion::Ready(())),
                StartMode::Pending => Ok(Completion::Pending),
                StartMode::Fail => Err(PortError::new(
                    PortErrorKind::Unavailable,
                    "recorder executable is unavailable",
                )),
            }
        }

        fn stop(
            &mut self,
            _session: &RecordingSession,
        ) -> Result<Completion<CaptureArtifact>, PortError> {
            self.stops += 1;
            match self.stop_mode {
                StopMode::Ready => Ok(Completion::Ready(artifact())),
                StopMode::Pending => Ok(Completion::Pending),
            }
        }
    }

    #[derive(Clone, Copy)]
    enum TranscriptionMode {
        Ready,
        Pending,
    }

    struct FakeTranscription {
        mode: TranscriptionMode,
        starts: usize,
        completion: Option<TranscriptionCompletion>,
        installing_model: bool,
        language: String,
    }

    impl TranscriptionPort for FakeTranscription {
        fn transcribe(
            &mut self,
            _recording: &RecordingFile,
        ) -> Result<Completion<TranscriptionOutput>, PortError> {
            self.starts += 1;
            match self.mode {
                TranscriptionMode::Ready => Ok(Completion::Ready(transcript())),
                TranscriptionMode::Pending => Ok(Completion::Pending),
            }
        }

        fn poll_completion(&mut self) -> Option<TranscriptionCompletion> {
            self.completion.take()
        }

        fn model_status(&mut self) -> Result<ModelStatus, PortError> {
            Ok(fake_model_status())
        }

        fn install_quality_model(&mut self) -> Result<Completion<ModelInstallOutcome>, PortError> {
            if self.installing_model {
                return Err(PortError::new(
                    PortErrorKind::Conflict,
                    "quality model installation is already in progress",
                ));
            }
            self.installing_model = true;
            Ok(Completion::Pending)
        }

        fn set_language(&mut self, language: &str) -> Result<(), PortError> {
            self.language = language.to_owned();
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeAnnotations {
        operations: Vec<String>,
    }

    impl AnnotationPort for FakeAnnotations {
        fn set_enabled(
            &mut self,
            _recording_id: &RecordingId,
            enabled: bool,
        ) -> Result<(), PortError> {
            self.operations.push(format!("enabled:{enabled}"));
            Ok(())
        }

        fn set_tool(
            &mut self,
            _recording_id: &RecordingId,
            tool: AnnotationTool,
        ) -> Result<(), PortError> {
            self.operations.push(format!("tool:{tool:?}"));
            Ok(())
        }

        fn undo(&mut self, _recording_id: &RecordingId) -> Result<(), PortError> {
            self.operations.push("undo".to_owned());
            Ok(())
        }

        fn clear(&mut self, _recording_id: &RecordingId) -> Result<(), PortError> {
            self.operations.push("clear".to_owned());
            Ok(())
        }

        fn finish(
            &mut self,
            _recording_id: &RecordingId,
        ) -> Result<Option<AnnotationFile>, PortError> {
            self.operations.push("finish".to_owned());
            Ok(None)
        }
    }

    #[derive(Default)]
    struct FakeStorage {
        recording_saves: usize,
        transcription_saves: usize,
        projects: Vec<ProjectFile>,
        recordings: Vec<RecordingFile>,
        deleted: Vec<RecordingId>,
        retry_candidates: VecDeque<RecordingFile>,
        pending_marks: usize,
        failed_marks: usize,
        settings: AppSettings,
        cleanup_calls: Vec<ProjectId>,
    }

    #[allow(clippy::default_trait_access)]
    impl StoragePort for FakeStorage {
        fn load_settings(&mut self) -> Result<AppSettings, PortError> {
            Ok(self.settings.clone())
        }

        fn save_settings(&mut self, settings: &AppSettings) -> Result<(), PortError> {
            self.settings.clone_from(settings);
            Ok(())
        }

        fn save_timeline_notes(
            &mut self,
            recording: &RecordingFile,
            notes: &[TimelineNote],
        ) -> Result<RecordingFile, PortError> {
            let Some(stored) = self.recordings.iter_mut().find(|candidate| {
                candidate.id == recording.id && candidate.project_id == recording.project_id
            }) else {
                return Err(PortError::new(
                    PortErrorKind::NotFound,
                    "recording was not found",
                ));
            };
            stored.timeline_notes = notes.to_vec();
            Ok(stored.clone())
        }

        fn cleanup_merged_videos(
            &mut self,
            project_id: &ProjectId,
        ) -> Result<CleanupSummary, PortError> {
            self.cleanup_calls.push(project_id.clone());
            Ok(CleanupSummary {
                removed_files: 2,
                freed_bytes: 42,
                cleaned_branches: vec!["feature/done".to_owned()],
                default_branch: Some("main".to_owned()),
                message: "Removed 2 merged videos.".to_owned(),
            })
        }

        fn load_projects(&mut self) -> Result<Vec<ProjectFile>, PortError> {
            Ok(self.projects.clone())
        }

        fn load_recordings(&mut self) -> Result<Vec<RecordingFile>, PortError> {
            Ok(self.recordings.clone())
        }

        fn poll_transcription_retry(&mut self) -> Option<Result<RecordingFile, PortError>> {
            self.retry_candidates.pop_front().map(Ok)
        }

        fn add_project(
            &mut self,
            path: &str,
            name: Option<&str>,
        ) -> Result<ProjectFile, PortError> {
            let project = ProjectFile {
                id: ProjectId::new("linked").unwrap_or_else(|_| unreachable!("valid test ID")),
                name: name.unwrap_or("Linked").to_owned(),
                created_at: Utc
                    .timestamp_opt(0, 0)
                    .single()
                    .unwrap_or_else(|| unreachable!("Unix epoch is valid")),
                source_path: Some(path.to_owned()),
            };
            self.projects.push(project.clone());
            Ok(project)
        }

        fn create_project(&mut self, name: &str) -> Result<ProjectFile, PortError> {
            let project = ProjectFile {
                id: ProjectId::new("created").unwrap_or_else(|_| unreachable!("valid test ID")),
                name: name.to_owned(),
                created_at: Utc
                    .timestamp_opt(0, 0)
                    .single()
                    .unwrap_or_else(|| unreachable!("Unix epoch is valid")),
                source_path: None,
            };
            self.projects.push(project.clone());
            Ok(project)
        }

        fn remove_project(&mut self, project_id: &ProjectId) -> Result<(), PortError> {
            self.projects.retain(|project| &project.id != project_id);
            Ok(())
        }

        fn delete_recording(&mut self, recording: &RecordingFile) -> Result<(), PortError> {
            self.deleted.push(recording.id.clone());
            self.recordings.retain(|candidate| {
                candidate.id != recording.id || candidate.project_id != recording.project_id
            });
            Ok(())
        }

        fn save_recording(
            &mut self,
            session: &RecordingSession,
            artifact: &CaptureArtifact,
            _annotations: Option<&AnnotationFile>,
        ) -> Result<RecordingFile, PortError> {
            self.recording_saves += 1;
            let project_id = session.project_id.clone().unwrap_or_else(|| {
                ProjectId::new(dicta_core::GENERAL_PROJECT_ID)
                    .unwrap_or_else(|_| unreachable!("core general project ID must be valid"))
            });
            let recording = RecordingFile {
                id: session.recording_id.clone(),
                project_id,
                video_path: artifact.path.display().to_string(),
                metadata_path: String::new(),
                note: session.note.clone().unwrap_or_default(),
                recording_scope: RecordingScope::Unprojected,
                git_branch: None,
                started_at: None,
                ended_at: None,
                duration_seconds: Some(artifact.duration.as_secs_f64()),
                size_bytes: None,
                success: true,
                transcript: None,
                transcript_path: None,
                transcript_segments: Vec::new(),
                transcription_status: TranscriptionStatus::Pending,
                transcription_error: None,
                transcription_language: None,
                poster_path: None,
                annotation_path: None,
                timeline_notes: Vec::new(),
                extra: Default::default(),
            };
            self.recordings.push(recording.clone());
            Ok(recording)
        }

        fn save_transcription(
            &mut self,
            _recording_id: &RecordingId,
            _output: &TranscriptionOutput,
        ) -> Result<(), PortError> {
            self.transcription_saves += 1;
            Ok(())
        }

        fn mark_transcription_pending(
            &mut self,
            _recording_id: &RecordingId,
        ) -> Result<(), PortError> {
            self.pending_marks += 1;
            Ok(())
        }

        fn mark_transcription_failed(
            &mut self,
            _recording_id: &RecordingId,
            _message: &str,
        ) -> Result<(), PortError> {
            self.failed_marks += 1;
            Ok(())
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> SystemTime {
            UNIX_EPOCH + Duration::from_hours(500_000)
        }
    }

    struct FixedIds {
        next: u32,
    }

    impl IdSource for FixedIds {
        fn next_recording_id(&mut self, _now: SystemTime) -> Result<RecordingId, PortError> {
            self.next += 1;
            RecordingId::new(format!("recording-{}", self.next)).map_err(|error| {
                PortError::new(PortErrorKind::Internal, format!("ID error: {error}"))
            })
        }
    }

    type TestRuntime =
        Runtime<FakeCapture, FakeTranscription, FakeAnnotations, FakeStorage, FixedClock, FixedIds>;

    fn runtime(
        start_mode: StartMode,
        stop_mode: StopMode,
        transcription_mode: TranscriptionMode,
        transcribe: bool,
    ) -> TestRuntime {
        Runtime::new(
            FakeCapture {
                start_mode,
                stop_mode,
                starts: 0,
                stops: 0,
            },
            FakeTranscription {
                mode: transcription_mode,
                starts: 0,
                completion: None,
                installing_model: false,
                language: "auto".to_owned(),
            },
            FakeAnnotations::default(),
            FakeStorage::default(),
            FixedClock,
            FixedIds { next: 0 },
            RuntimeConfig {
                transcribe_after_recording: transcribe,
            },
        )
    }

    fn artifact() -> CaptureArtifact {
        CaptureArtifact {
            path: PathBuf::from("/tmp/recording-1.mp4"),
            duration: Duration::from_millis(2_500),
            backend: CaptureBackend::GpuScreenRecorder,
            output_name: "DP-1".to_owned(),
            geometry: Geometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale_milli: 1_000,
            encoded_pixel_size: (1920, 1080),
        }
    }

    fn transcript() -> TranscriptionOutput {
        TranscriptionOutput::new(
            "fixed the overflow".to_owned(),
            vec![TranscriptSegment {
                start_seconds: 0.0,
                end_seconds: 2.5,
                text: "fixed the overflow".to_owned(),
            }],
        )
    }

    fn fake_model_status() -> ModelStatus {
        ModelStatus {
            active_model: Some(dicta_transcribe::PreparedModel::new(
                dicta_transcribe::ModelKind::Compact,
                PathBuf::from("/models/ggml-base.bin"),
            )),
            quality: dicta_transcribe::ManagedModelStatus {
                kind: dicta_transcribe::ModelKind::LargeV3Turbo,
                path: PathBuf::from("/models/ggml-large-v3-turbo-q5_0.bin"),
                state: ModelFileState::Missing,
                size_bytes: 0,
                expected_download_bytes: dicta_transcribe::LARGE_V3_TURBO_DOWNLOAD_BYTES,
                detail: "quality model is not installed".to_owned(),
            },
            install_progress: None,
            install_error: None,
        }
    }

    fn catalog_project(id: &str, name: &str) -> ProjectFile {
        ProjectFile {
            id: ProjectId::new(id).unwrap_or_else(|_| unreachable!("static project ID is valid")),
            name: name.to_owned(),
            created_at: Utc
                .timestamp_opt(1_800_000_000, 0)
                .single()
                .unwrap_or_else(|| unreachable!("static timestamp is valid")),
            source_path: Some(format!("/projects/{id}")),
        }
    }

    #[allow(clippy::default_trait_access)]
    fn catalog_recording(
        id: &str,
        project: &str,
        started_at: i64,
        branch: Option<&str>,
        transcription_status: TranscriptionStatus,
    ) -> RecordingFile {
        RecordingFile {
            id: RecordingId::new(id)
                .unwrap_or_else(|_| unreachable!("static recording ID is valid")),
            project_id: ProjectId::new(project)
                .unwrap_or_else(|_| unreachable!("static project ID is valid")),
            video_path: format!("/recordings/{id}.mp4"),
            metadata_path: format!("/recordings/{id}.json"),
            note: format!("note for {id}"),
            recording_scope: RecordingScope::Branch,
            git_branch: branch.map(str::to_owned),
            started_at: Some(
                Utc.timestamp_opt(started_at, 0)
                    .single()
                    .unwrap_or_else(|| unreachable!("static timestamp is valid")),
            ),
            ended_at: None,
            duration_seconds: Some(42.0),
            size_bytes: Some(1_024),
            success: true,
            transcript: Some(format!("transcript for {id}")),
            transcript_path: Some(format!("/recordings/{id}.md")),
            transcript_segments: Vec::new(),
            transcription_status,
            transcription_error: None,
            transcription_language: Some("en".to_owned()),
            poster_path: None,
            annotation_path: None,
            timeline_notes: Vec::new(),
            extra: Default::default(),
        }
    }

    fn response(output: &ControlOutput) -> &Response {
        match &output.response.payload {
            ResponsePayload::Success { result } => result,
            ResponsePayload::Failure { error } => panic!("unexpected response error: {error}"),
        }
    }

    fn request(command: ControlCommand) -> RequestEnvelope {
        RequestEnvelope::new(
            RequestId::new(
                NonZeroU64::new(1).unwrap_or_else(|| unreachable!("one is a non-zero request ID")),
            ),
            command,
        )
    }

    fn error_code(output: &ControlOutput) -> Option<ErrorCode> {
        match &output.response.payload {
            ResponsePayload::Failure { error } => Some(error.code),
            ResponsePayload::Success { .. } => None,
        }
    }

    fn start(runtime: &mut TestRuntime) -> ControlOutput {
        runtime.handle(request(ControlCommand::RecordStart {
            project: Some("dicta".to_owned()),
            note: Some("explain the bug".to_owned()),
        }))
    }

    #[test]
    fn synchronous_flow_records_annotates_saves_transcribes_and_returns_idle() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            true,
        );
        assert_eq!(error_code(&start(&mut runtime)), None);
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Recording);

        for command in [
            ControlCommand::AnnotationEnable,
            ControlCommand::AnnotationTool {
                tool: AnnotationTool::Arrow,
            },
            ControlCommand::AnnotationUndo,
            ControlCommand::AnnotationClear,
        ] {
            assert_eq!(error_code(&runtime.handle(request(command))), None);
        }
        assert!(runtime.snapshot().status.annotations_enabled);
        assert_eq!(
            runtime.snapshot().status.annotation_tool,
            Some(AnnotationTool::Arrow)
        );

        assert_eq!(
            error_code(&runtime.handle(request(ControlCommand::RecordStop))),
            None
        );
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Idle);
        assert_eq!(runtime.capture.starts, 1);
        assert_eq!(runtime.capture.stops, 1);
        assert_eq!(runtime.storage.recording_saves, 1);
        assert_eq!(runtime.storage.transcription_saves, 1);
        assert_eq!(runtime.transcription.starts, 1);
        assert_eq!(
            runtime.annotations.operations,
            [
                "enabled:true",
                "tool:Pen",
                "tool:Arrow",
                "undo",
                "clear",
                "finish"
            ]
        );

        let sequences: Vec<_> = runtime.events().iter().map(event_sequence).collect();
        assert_eq!(sequences, (1..=sequences.len() as u64).collect::<Vec<_>>());
    }

    #[test]
    fn pending_start_rejects_stale_completion_without_mutation() {
        let mut runtime = runtime(
            StartMode::Pending,
            StopMode::Ready,
            TranscriptionMode::Ready,
            true,
        );
        start(&mut runtime);
        let before = runtime.snapshot();
        let event_count = runtime.events().len();
        let stale = RecordingId::new("old-recording")
            .unwrap_or_else(|_| unreachable!("static test ID is valid"));
        let error = runtime.complete_capture_start(stale, Ok(())).unwrap_err();
        assert_eq!(error.protocol_error().code, ErrorCode::Conflict);
        assert_eq!(runtime.snapshot(), before);
        assert_eq!(runtime.events().len(), event_count);

        let current = RecordingId::new("recording-1")
            .unwrap_or_else(|_| unreachable!("static test ID is valid"));
        runtime.complete_capture_start(current, Ok(())).unwrap();
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Recording);
    }

    #[test]
    fn pending_ports_complete_the_full_flow_without_an_executor() {
        let mut runtime = runtime(
            StartMode::Pending,
            StopMode::Pending,
            TranscriptionMode::Pending,
            true,
        );
        start(&mut runtime);
        let recording_id = RecordingId::new("recording-1")
            .unwrap_or_else(|_| unreachable!("static test ID is valid"));
        runtime
            .complete_capture_start(recording_id.clone(), Ok(()))
            .unwrap();
        runtime.handle(request(ControlCommand::AnnotationEnable));
        runtime.handle(request(ControlCommand::AnnotationDisable));
        runtime.handle(request(ControlCommand::RecordStop));
        runtime
            .complete_capture_stop(recording_id.clone(), Ok(artifact()))
            .unwrap();
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Transcribing);
        runtime.transcription.completion = Some(TranscriptionCompletion {
            recording_id,
            result: Ok(transcript()),
        });
        assert!(runtime.poll_background().unwrap());
        assert!(!runtime.poll_background().unwrap());
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Idle);
        assert_eq!(runtime.storage.recording_saves, 1);
        assert_eq!(runtime.storage.transcription_saves, 1);
    }

    #[test]
    fn concurrent_start_and_busy_toggle_are_protocol_conflicts() {
        let mut runtime = runtime(
            StartMode::Pending,
            StopMode::Ready,
            TranscriptionMode::Ready,
            true,
        );
        start(&mut runtime);
        let before = runtime.snapshot();
        let duplicate = start(&mut runtime);
        assert_eq!(error_code(&duplicate), Some(ErrorCode::Conflict));
        assert_eq!(runtime.snapshot(), before);

        let toggle = runtime.handle(request(ControlCommand::RecordToggle));
        assert_eq!(error_code(&toggle), Some(ErrorCode::Conflict));
        assert_eq!(runtime.snapshot(), before);
    }

    #[test]
    fn stale_stop_completion_does_not_touch_annotations_or_storage() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Pending,
            TranscriptionMode::Ready,
            true,
        );
        start(&mut runtime);
        runtime.handle(request(ControlCommand::RecordStop));
        let before = runtime.snapshot();
        let stale = RecordingId::new("old-recording")
            .unwrap_or_else(|_| unreachable!("static test ID is valid"));
        let error = runtime
            .complete_capture_stop(stale, Ok(artifact()))
            .unwrap_err();
        assert_eq!(error.protocol_error().code, ErrorCode::Conflict);
        assert_eq!(runtime.snapshot(), before);
        assert_eq!(runtime.storage.recording_saves, 0);
        assert!(runtime.annotations.operations.is_empty());
    }

    #[test]
    fn stale_transcription_completion_does_not_touch_storage() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            true,
        );
        start(&mut runtime);
        runtime.handle(request(ControlCommand::RecordStop));
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Transcribing);
        let before = runtime.snapshot();
        let stale = RecordingId::new("old-recording")
            .unwrap_or_else(|_| unreachable!("static test ID is valid"));
        let error = runtime
            .complete_transcription(stale, Ok(transcript()))
            .unwrap_err();
        assert_eq!(error.protocol_error().code, ErrorCode::Conflict);
        assert_eq!(runtime.snapshot(), before);
        assert_eq!(runtime.storage.transcription_saves, 0);
    }

    #[test]
    fn recorder_start_failure_enters_failed_and_uses_stable_error_code() {
        let mut runtime = runtime(
            StartMode::Fail,
            StopMode::Ready,
            TranscriptionMode::Ready,
            true,
        );
        let output = start(&mut runtime);
        assert_eq!(error_code(&output), Some(ErrorCode::Unavailable));
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Failed);
        assert!(output
            .events
            .iter()
            .any(|event| matches!(event.event, ControlEvent::Failed { .. })));
    }

    #[test]
    fn events_command_filters_the_immutable_event_log() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        start(&mut runtime);
        let before = runtime.events().to_vec();
        let output = runtime.handle(request(ControlCommand::Events {
            since_sequence: Some(1),
        }));
        assert_eq!(error_code(&output), None);
        assert_eq!(output.events.len(), before.len() - 1);
        assert_eq!(runtime.events(), before);
    }

    #[test]
    fn ui_show_is_a_monotonic_observable_request_without_state_mutation() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        let before = runtime.snapshot();
        let output = runtime.handle(request(ControlCommand::UiShow));
        assert_eq!(response(&output), &Response::Accepted);
        assert!(matches!(
            output.events.as_slice(),
            [EventEnvelope {
                event: ControlEvent::UiShowRequested { sequence: 1 },
                ..
            }]
        ));
        let after = runtime.snapshot();
        assert_eq!(after.app, before.app);
        assert_eq!(after.status, before.status);
        assert_eq!(after.last_event_sequence, 1);
    }

    #[test]
    fn model_status_and_install_are_typed_nonblocking_and_conflict_safe() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            true,
        );
        let status = runtime.handle(request(ControlCommand::ModelStatus));
        let Response::ModelStatus(status) = response(&status) else {
            panic!("model status returned another response type");
        };
        assert_eq!(status.quality_state, ModelState::Missing);
        assert_eq!(status.active_model.as_deref(), Some("Compact · base"));

        let started = runtime.handle(request(ControlCommand::ModelInstall {
            model: ModelTier::Quality,
        }));
        assert_eq!(response(&started), &Response::ModelInstallStarted);
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Idle);

        let duplicate = runtime.handle(request(ControlCommand::ModelInstall {
            model: ModelTier::Quality,
        }));
        assert_eq!(error_code(&duplicate), Some(ErrorCode::Conflict));

        runtime.handle(request(ControlCommand::RecordStart {
            project: None,
            note: None,
        }));
        let busy = runtime.handle(request(ControlCommand::ModelInstall {
            model: ModelTier::Quality,
        }));
        assert_eq!(error_code(&busy), Some(ErrorCode::Conflict));
    }

    #[test]
    fn settings_round_trip_validate_and_apply_language_without_restart() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            false,
        );
        let initial = runtime.handle(request(ControlCommand::SettingsGet));
        assert_eq!(
            response(&initial),
            &Response::Settings(AppSettings::default())
        );

        runtime.handle(request(ControlCommand::SettingsSetShortcut {
            shortcut_id: "control_space".to_owned(),
        }));
        runtime.handle(request(ControlCommand::SettingsSetCleanup {
            enabled: false,
        }));
        runtime.handle(request(ControlCommand::SettingsSetBranchLocking {
            enabled: false,
        }));
        let language = runtime.handle(request(ControlCommand::SettingsSetLanguage {
            language: "nl".to_owned(),
        }));
        let Response::Settings(settings) = response(&language) else {
            panic!("expected settings response");
        };
        assert_eq!(settings.shortcut_id, "control_space");
        assert!(!settings.cleanup_merged_videos);
        assert!(!settings.branch_locking);
        assert_eq!(settings.transcription_language, "nl");
        assert_eq!(runtime.transcription.language, "nl");

        let path = runtime.handle(request(ControlCommand::SettingsSetGeneralPath {
            path: Some("/tmp/dicta-general".to_owned()),
        }));
        let Response::Settings(settings) = response(&path) else {
            panic!("expected settings response");
        };
        assert_eq!(settings.general_path.as_deref(), Some("/tmp/dicta-general"));

        assert_eq!(
            error_code(
                &runtime.handle(request(ControlCommand::SettingsSetLanguage {
                    language: "xx".to_owned(),
                }))
            ),
            Some(ErrorCode::InvalidRequest)
        );
        assert_eq!(
            error_code(
                &runtime.handle(request(ControlCommand::SettingsSetGeneralPath {
                    path: Some("relative".to_owned()),
                }))
            ),
            Some(ErrorCode::InvalidRequest)
        );

        runtime.handle(request(ControlCommand::RecordStart {
            project: None,
            note: None,
        }));
        assert_eq!(
            error_code(
                &runtime.handle(request(ControlCommand::SettingsSetBranchLocking {
                    enabled: true,
                }))
            ),
            Some(ErrorCode::Conflict)
        );
    }

    #[test]
    fn merged_video_cleanup_uses_selected_project_and_rejects_busy_state() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        runtime.storage.projects = vec![catalog_project("demo", "Demo")];
        runtime.handle(request(ControlCommand::ProjectSelect {
            project: "demo".to_owned(),
        }));
        let cleanup = runtime.handle(request(ControlCommand::SettingsCleanupMerged {
            project: None,
        }));
        let Response::Cleanup(summary) = response(&cleanup) else {
            panic!("expected cleanup response");
        };
        assert_eq!(summary.removed_files, 2);
        assert_eq!(runtime.storage.cleanup_calls[0].as_str(), "demo");

        runtime.handle(request(ControlCommand::RecordStart {
            project: None,
            note: None,
        }));
        assert_eq!(
            error_code(
                &runtime.handle(request(ControlCommand::SettingsCleanupMerged {
                    project: Some("demo".to_owned()),
                }))
            ),
            Some(ErrorCode::Conflict)
        );
    }

    #[test]
    fn project_reads_and_selection_use_the_persisted_catalog() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        runtime.storage.projects = vec![
            catalog_project("beta", "Beta"),
            catalog_project("alpha", "Alpha"),
        ];

        let listed = runtime.handle(request(ControlCommand::ProjectList));
        let Response::Projects(projects) = response(&listed) else {
            panic!("project list returned another response type");
        };
        assert_eq!(
            projects
                .iter()
                .map(|project| project.id.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        assert!(projects.iter().all(|project| !project.selected));

        let selected = runtime.handle(request(ControlCommand::ProjectSelect {
            project: "beta".to_owned(),
        }));
        assert_eq!(response(&selected), &Response::Accepted);
        assert_eq!(selected.events.len(), 1);
        let current = runtime.handle(request(ControlCommand::ProjectCurrent));
        let Response::Project(Some(project)) = response(&current) else {
            panic!("current project returned another response type");
        };
        assert_eq!(project.id, "beta");
        assert!(project.selected);
        assert_eq!(project.path.as_deref(), Some("/projects/beta"));
    }

    #[test]
    fn project_mutations_select_refresh_and_safely_clear_removed_selection() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        runtime.storage.projects = vec![catalog_project(dicta_core::GENERAL_PROJECT_ID, "General")];

        let linked = runtime.handle(request(ControlCommand::ProjectAdd {
            path: "/projects/linked".to_owned(),
            name: Some("Linked Project".to_owned()),
        }));
        assert_eq!(response(&linked), &Response::Accepted);
        assert_eq!(runtime.snapshot().status.project, None);
        let refreshed = runtime.handle(request(ControlCommand::ProjectRefresh {
            project: "linked".to_owned(),
        }));
        let Response::Project(Some(project)) = response(&refreshed) else {
            panic!("project refresh returned another response type");
        };
        assert_eq!(project.name, "Linked Project");
        assert!(!project.selected);

        let selected = runtime.handle(request(ControlCommand::ProjectSelect {
            project: "linked".to_owned(),
        }));
        assert_eq!(response(&selected), &Response::Accepted);
        assert_eq!(runtime.snapshot().status.project.as_deref(), Some("linked"));

        let removed = runtime.handle(request(ControlCommand::ProjectRemove {
            project: "linked".to_owned(),
        }));
        assert_eq!(response(&removed), &Response::Accepted);
        assert_eq!(runtime.snapshot().status.project, None);
        let general = runtime.handle(request(ControlCommand::ProjectRemove {
            project: dicta_core::GENERAL_PROJECT_ID.to_owned(),
        }));
        assert_eq!(error_code(&general), Some(ErrorCode::InvalidRequest));

        let created = runtime.handle(request(ControlCommand::ProjectCreate {
            name: "Created Project".to_owned(),
        }));
        assert_eq!(response(&created), &Response::Accepted);
        assert_eq!(
            runtime.snapshot().status.project.as_deref(),
            Some("created")
        );
    }

    #[test]
    fn project_linking_during_recording_preserves_the_recording_destination() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        assert_eq!(error_code(&start(&mut runtime)), None);
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Recording);
        assert_eq!(runtime.snapshot().status.project.as_deref(), Some("dicta"));

        let linked = runtime.handle(request(ControlCommand::ProjectAdd {
            path: "/projects/linked".to_owned(),
            name: Some("Linked Project".to_owned()),
        }));

        assert_eq!(response(&linked), &Response::Accepted);
        assert_eq!(runtime.snapshot().status.project.as_deref(), Some("dicta"));
        let listed = runtime.handle(request(ControlCommand::ProjectList));
        let Response::Projects(projects) = response(&listed) else {
            panic!("project list returned another response type");
        };
        assert!(projects.iter().any(|project| project.id == "linked"));
        assert!(projects.iter().all(|project| !project.selected));
    }

    #[test]
    fn recording_list_filters_sorts_limits_and_show_returns_core_details() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        runtime.storage.recordings = vec![
            catalog_recording(
                "alpha-old",
                "alpha",
                1_700_000_000,
                Some("main"),
                TranscriptionStatus::Pending,
            ),
            catalog_recording(
                "alpha-new",
                "alpha",
                1_800_000_000,
                Some("main"),
                TranscriptionStatus::Complete,
            ),
            catalog_recording(
                "beta-newest",
                "beta",
                1_900_000_000,
                Some("feature"),
                TranscriptionStatus::Failed,
            ),
        ];

        let listed = runtime.handle(request(ControlCommand::RecordingList {
            project: Some("alpha".to_owned()),
            branch: Some("main".to_owned()),
            limit: Some(1),
        }));
        let Response::Recordings(recordings) = response(&listed) else {
            panic!("recording list returned another response type");
        };
        assert_eq!(recordings.len(), 1);
        assert_eq!(recordings[0].id, "alpha-new");
        assert_eq!(recordings[0].transcription, TranscriptionState::Complete);

        let shown = runtime.handle(request(ControlCommand::RecordingShow {
            recording: RecordingSelector::Latest,
        }));
        let Response::RecordingDetails(recording) = response(&shown) else {
            panic!("recording show returned another response type");
        };
        assert_eq!(recording.id.as_str(), "beta-newest");
        assert_eq!(
            recording.transcript.as_deref(),
            Some("transcript for beta-newest")
        );

        let before = runtime.snapshot();
        let opened = runtime.handle(request(ControlCommand::RecordingOpen {
            recording: RecordingSelector::Id("alpha-new".to_owned()),
        }));
        assert_eq!(response(&opened), &Response::Accepted);
        let after = runtime.snapshot();
        assert_eq!(after.app, before.app);
        assert_eq!(after.status, before.status);
        assert_eq!(after.last_event_sequence, 1);
        assert_eq!(
            opened.events,
            vec![EventEnvelope::new(ControlEvent::UiRecordingRequested {
                sequence: 1,
                recording_id: "alpha-new".to_owned(),
            })]
        );
    }

    #[test]
    #[allow(clippy::default_trait_access)]
    fn timeline_notes_are_validated_sorted_and_persisted_atomically() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        runtime.storage.recordings = vec![catalog_recording(
            "timeline-demo",
            "alpha",
            1_800_000_000,
            Some("main"),
            TranscriptionStatus::Complete,
        )];
        runtime.storage.recordings[0].duration_seconds = Some(30.0);
        let created_at = Utc
            .timestamp_opt(1_800_000_100, 0)
            .single()
            .unwrap_or_else(|| unreachable!("test timestamp is valid"));
        let notes = vec![
            TimelineNote {
                id: "later".to_owned(),
                timestamp_seconds: 18.5,
                text: "Later note".to_owned(),
                created_at,
                source: "typed".to_owned(),
                extra: Default::default(),
            },
            TimelineNote {
                id: "earlier".to_owned(),
                timestamp_seconds: 3.0,
                text: "Earlier note".to_owned(),
                created_at,
                source: "voice".to_owned(),
                extra: Default::default(),
            },
        ];

        let updated = runtime.handle(request(ControlCommand::RecordingSetTimelineNotes {
            recording: RecordingSelector::Id("timeline-demo".to_owned()),
            notes,
        }));
        let Response::RecordingDetails(recording) = response(&updated) else {
            panic!("timeline update returned another response type");
        };
        assert_eq!(recording.timeline_notes[0].id, "earlier");
        assert_eq!(recording.timeline_notes[1].id, "later");
        assert_eq!(
            runtime.storage.recordings[0].timeline_notes,
            recording.timeline_notes
        );

        let before = runtime.storage.recordings[0].clone();
        let duplicate = TimelineNote {
            id: "duplicate".to_owned(),
            timestamp_seconds: 31.0,
            text: "Past the end".to_owned(),
            created_at,
            source: "typed".to_owned(),
            extra: Default::default(),
        };
        let rejected = runtime.handle(request(ControlCommand::RecordingSetTimelineNotes {
            recording: RecordingSelector::Id("timeline-demo".to_owned()),
            notes: vec![duplicate.clone(), duplicate],
        }));
        assert!(matches!(
            rejected.response.payload,
            ResponsePayload::Failure { .. }
        ));
        assert_eq!(runtime.storage.recordings[0], before);
    }

    #[test]
    fn voice_note_completion_persists_at_timestamp_and_cleans_audio() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            false,
        );
        let mut recording = catalog_recording(
            "voice-demo",
            "alpha",
            1_800_000_000,
            Some("main"),
            TranscriptionStatus::Complete,
        );
        recording.duration_seconds = Some(30.0);
        runtime.storage.recordings = vec![recording.clone()];
        let audio = std::env::temp_dir().join(format!(
            "dicta-runtime-voice-{}-{:?}.wav",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(&audio, vec![0_u8; 128]).unwrap();
        runtime.pending_voice_note = Some(PendingVoiceNote {
            recording: recording.clone(),
            note_id: "voice-note-1".to_owned(),
            timestamp_seconds: 12.5,
            audio_path: audio.clone(),
        });
        runtime.voice_note_status = VoiceNoteStatus {
            state: VoiceNoteState::Processing,
            recording_id: Some(recording.id.to_string()),
            note_id: Some("voice-note-1".to_owned()),
            message: "processing".to_owned(),
        };
        runtime.transcription.completion = Some(TranscriptionCompletion {
            recording_id: recording.id,
            result: Ok(transcript()),
        });

        assert!(runtime.poll_background().unwrap());
        assert!(!audio.exists());
        let note = &runtime.storage.recordings[0].timeline_notes[0];
        assert_eq!(note.id, "voice-note-1");
        assert!((note.timestamp_seconds - 12.5).abs() < f64::EPSILON);
        assert_eq!(note.source, "voice");
        assert_eq!(note.text, "fixed the overflow");
        assert_eq!(runtime.voice_note_status.state, VoiceNoteState::Complete);
    }

    #[test]
    fn cancelled_voice_note_rejects_stale_completion_without_metadata_mutation() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            false,
        );
        let recording = catalog_recording(
            "voice-cancel",
            "alpha",
            1_800_000_000,
            Some("main"),
            TranscriptionStatus::Complete,
        );
        runtime.storage.recordings = vec![recording.clone()];
        let audio = std::env::temp_dir().join(format!(
            "dicta-runtime-voice-cancel-{}-{:?}.wav",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::write(&audio, vec![0_u8; 128]).unwrap();
        runtime.pending_voice_note = Some(PendingVoiceNote {
            recording: recording.clone(),
            note_id: "voice-note-cancelled".to_owned(),
            timestamp_seconds: 3.0,
            audio_path: audio.clone(),
        });
        runtime.voice_note_status.state = VoiceNoteState::Processing;

        let cancelled = runtime.handle(request(ControlCommand::RecordingVoiceNoteCancel));
        let Response::VoiceNote(status) = response(&cancelled) else {
            panic!("voice cancellation returned another response type");
        };
        assert_eq!(status.state, VoiceNoteState::Cancelling);
        assert!(!audio.exists());
        runtime.transcription.completion = Some(TranscriptionCompletion {
            recording_id: recording.id,
            result: Ok(transcript()),
        });
        assert!(runtime.poll_background().unwrap());
        assert!(runtime.storage.recordings[0].timeline_notes.is_empty());
        assert_eq!(runtime.voice_note_status.state, VoiceNoteState::Idle);
    }

    #[test]
    fn online_context_uses_the_catalog_and_project_filter() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        runtime.storage.projects = vec![
            catalog_project("alpha", "Alpha project"),
            catalog_project("beta", "Beta project"),
        ];
        runtime.storage.recordings = vec![
            catalog_recording(
                "shared-id",
                "alpha",
                1_800_000_000,
                Some("main"),
                TranscriptionStatus::Complete,
            ),
            catalog_recording(
                "shared-id",
                "beta",
                1_900_000_000,
                Some("feature"),
                TranscriptionStatus::Complete,
            ),
        ];

        let ambiguous = runtime.handle(request(ControlCommand::Context {
            recording: RecordingSelector::Id("shared-id".to_owned()),
            project: None,
            copy: false,
        }));
        assert_eq!(error_code(&ambiguous), Some(ErrorCode::Conflict));

        let selected = runtime.handle(request(ControlCommand::Context {
            recording: RecordingSelector::Id("shared-id".to_owned()),
            project: Some("alpha".to_owned()),
            copy: true,
        }));
        let Response::Context { text } = response(&selected) else {
            panic!("context returned another response type");
        };
        assert!(text.contains("Project: Alpha project (`alpha`)"));
        assert!(text.contains("Branch: `main`"));
        assert!(text.contains("note for shared-id"));
        assert!(text.contains("transcript for shared-id"));
    }

    #[test]
    fn explicit_existing_recording_transcription_uses_normal_completion_flow() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            false,
        );
        runtime.storage.recordings = vec![catalog_recording(
            "existing",
            "alpha",
            1_800_000_000,
            Some("main"),
            TranscriptionStatus::Pending,
        )];
        let output = runtime.handle(request(ControlCommand::RecordingTranscribe {
            recording: RecordingSelector::Id("existing".to_owned()),
        }));
        assert_eq!(response(&output), &Response::Accepted);
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Transcribing);
        assert_eq!(runtime.transcription.starts, 1);

        let recording_id = RecordingId::new("existing")
            .unwrap_or_else(|_| unreachable!("static recording ID is valid"));
        runtime
            .complete_transcription(recording_id, Ok(transcript()))
            .unwrap();
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Idle);
        assert_eq!(runtime.storage.transcription_saves, 1);
    }

    #[test]
    fn startup_retry_candidate_is_started_only_from_idle_background_poll() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            true,
        );
        let recording = catalog_recording(
            "retry-me",
            "alpha",
            1_800_000_000,
            Some("main"),
            TranscriptionStatus::Pending,
        );
        runtime
            .storage
            .retry_candidates
            .push_back(recording.clone());

        assert!(runtime.poll_background().unwrap());
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Transcribing);
        assert_eq!(runtime.transcription.starts, 1);
        assert_eq!(runtime.storage.pending_marks, 1);

        runtime.transcription.completion = Some(TranscriptionCompletion {
            recording_id: recording.id,
            result: Ok(transcript()),
        });
        assert!(runtime.poll_background().unwrap());
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Idle);
        assert_eq!(runtime.storage.transcription_saves, 1);
    }

    #[test]
    fn ambiguous_recording_ids_are_stable_conflicts_without_state_changes() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Pending,
            false,
        );
        runtime.storage.recordings = vec![
            catalog_recording(
                "shared",
                "alpha",
                1_800_000_000,
                None,
                TranscriptionStatus::Pending,
            ),
            catalog_recording(
                "shared",
                "beta",
                1_900_000_000,
                None,
                TranscriptionStatus::Pending,
            ),
        ];
        let before = runtime.snapshot();
        let output = runtime.handle(request(ControlCommand::RecordingShow {
            recording: RecordingSelector::Id("shared".to_owned()),
        }));
        assert_eq!(error_code(&output), Some(ErrorCode::Conflict));
        assert_eq!(runtime.snapshot(), before);
    }

    #[test]
    fn recording_delete_resolves_catalog_entry_and_rejects_busy_state() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Pending,
            TranscriptionMode::Pending,
            false,
        );
        runtime.storage.recordings = vec![catalog_recording(
            "delete-me",
            "alpha",
            1_800_000_000,
            None,
            TranscriptionStatus::Complete,
        )];

        let started = runtime.handle(request(ControlCommand::RecordStart {
            project: None,
            note: None,
        }));
        assert_eq!(response(&started), &Response::Accepted);
        let busy = runtime.handle(request(ControlCommand::RecordingDelete {
            recording: RecordingSelector::Id("delete-me".to_owned()),
        }));
        assert_eq!(error_code(&busy), Some(ErrorCode::Conflict));
        assert!(runtime.storage.deleted.is_empty());

        let stopped = runtime.handle(request(ControlCommand::RecordStop));
        assert_eq!(response(&stopped), &Response::Accepted);
        let recording_id = RecordingId::new("recording-1")
            .unwrap_or_else(|_| unreachable!("static recording ID is valid"));
        runtime
            .complete_capture_stop(recording_id, Ok(artifact()))
            .unwrap();
        let deleted = runtime.handle(request(ControlCommand::RecordingDelete {
            recording: RecordingSelector::Id("delete-me".to_owned()),
        }));
        assert_eq!(response(&deleted), &Response::Accepted);
        assert_eq!(runtime.storage.deleted.len(), 1);
        assert!(runtime
            .storage
            .recordings
            .iter()
            .all(|recording| recording.id.as_str() != "delete-me"));
    }

    #[test]
    fn unsupported_version_does_not_change_state() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            true,
        );
        let before = runtime.snapshot();
        let mut incompatible = request(ControlCommand::Status);
        incompatible.version = PROTOCOL_VERSION + 1;
        let output = runtime.handle(incompatible);
        assert_eq!(error_code(&output), Some(ErrorCode::UnsupportedVersion));
        assert_eq!(runtime.snapshot(), before);
    }

    #[test]
    fn transcription_can_be_disabled_without_changing_the_recording_flow() {
        let mut runtime = runtime(
            StartMode::Ready,
            StopMode::Ready,
            TranscriptionMode::Ready,
            false,
        );
        start(&mut runtime);
        runtime.handle(request(ControlCommand::RecordStop));
        assert_eq!(runtime.snapshot().status.phase, AppPhase::Idle);
        assert_eq!(runtime.storage.recording_saves, 1);
        assert_eq!(runtime.transcription.starts, 0);
        assert_eq!(runtime.storage.transcription_saves, 0);
    }
}
