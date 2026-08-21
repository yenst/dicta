use super::*;
use crate::runtime::PendingVoiceNote;
use chrono::{TimeZone, Utc};
use dicta_capture::{CaptureArtifact, CaptureBackend, Geometry};
use dicta_control::{
    protocol::{AppPhase, ResponsePayload, TranscriptionState},
    AnnotationTool, CleanupSummary, Command as ControlCommand, ErrorCode, Event as ControlEvent,
    EventEnvelope, ModelState, ModelTier, RecordingSelector, RequestEnvelope, RequestId, Response,
    SettingsDocument, TimelineNoteDocument, VoiceNoteState, VoiceNoteStatus, PROTOCOL_VERSION,
};
use dicta_core::{
    storage::AppSettings, AnnotationFile, ProjectFile, ProjectId, RecordingFile, RecordingId,
    RecordingScope, TimelineNote, TranscriptSegment, TranscriptionStatus,
};
use dicta_engine::RecordingSession;
use dicta_transcribe::{ModelFileState, ModelInstallOutcome, ModelStatus, TranscriptionOutput};
use std::{
    collections::VecDeque,
    fs,
    num::NonZeroU64,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    fn set_enabled(&mut self, _recording_id: &RecordingId, enabled: bool) -> Result<(), PortError> {
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

    fn finish(&mut self, _recording_id: &RecordingId) -> Result<Option<AnnotationFile>, PortError> {
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

    fn add_project(&mut self, path: &str, name: Option<&str>) -> Result<ProjectFile, PortError> {
        let project = ProjectFile {
            id: ProjectId::new("linked").unwrap_or_else(|_| unreachable!("valid test ID")),
            name: name.unwrap_or("Linked").to_owned(),
            created_at: Utc
                .timestamp_opt(0, 0)
                .single()
                .unwrap_or_else(|| unreachable!("Unix epoch is valid")),
            source_path: Some(path.to_owned()),
            extra: serde_json::Map::new(),
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
            extra: serde_json::Map::new(),
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
            extra: serde_json::Map::new(),
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

    fn mark_transcription_pending(&mut self, _recording_id: &RecordingId) -> Result<(), PortError> {
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
        RecordingId::new(format!("recording-{}", self.next))
            .map_err(|error| PortError::new(PortErrorKind::Internal, format!("ID error: {error}")))
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
        extra: serde_json::Map::new(),
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
        id: RecordingId::new(id).unwrap_or_else(|_| unreachable!("static recording ID is valid")),
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
        extra: serde_json::Map::new(),
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

    let current =
        RecordingId::new("recording-1").unwrap_or_else(|_| unreachable!("static test ID is valid"));
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
    let recording_id =
        RecordingId::new("recording-1").unwrap_or_else(|_| unreachable!("static test ID is valid"));
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
fn retained_events_are_capped_without_rewriting_sequence_numbers() {
    let mut runtime = runtime(
        StartMode::Ready,
        StopMode::Ready,
        TranscriptionMode::Ready,
        false,
    );
    for _ in 0..(MAX_RETAINED_EVENTS + 24) {
        runtime.handle(request(ControlCommand::UiShow));
    }
    let events = runtime.events();
    assert_eq!(events.len(), MAX_RETAINED_EVENTS);
    let first = event_sequence(&events[0]);
    let last = event_sequence(events.last().expect("retained events"));
    assert!(first > 1, "oldest events must be dropped");
    assert_eq!(
        last,
        first + u64::try_from(MAX_RETAINED_EVENTS - 1).unwrap()
    );
    assert_eq!(runtime.snapshot().last_event_sequence, last);
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
        follow: false,
    }));
    assert_eq!(error_code(&output), None);
    assert_eq!(output.events.len(), before.len() - 1);
    assert_eq!(runtime.events(), before);
}

#[test]
fn events_follow_publishes_current_state_without_dropping_history() {
    let mut runtime = runtime(
        StartMode::Ready,
        StopMode::Ready,
        TranscriptionMode::Ready,
        false,
    );
    start(&mut runtime);
    let before = runtime.events().len();
    let output = runtime.handle(request(ControlCommand::Events {
        since_sequence: None,
        follow: true,
    }));
    assert_eq!(error_code(&output), None);
    assert_eq!(output.events.len(), before + 1);
    assert!(matches!(
        output.events.last().map(|event| &event.event),
        Some(ControlEvent::StateChanged { .. })
    ));
}

#[test]
fn settings_wire_document_matches_persisted_settings_json() {
    let settings = AppSettings::default();
    let document = crate::runtime::wire::settings_document(settings.clone());
    assert_eq!(
        serde_json::to_value(&settings).unwrap(),
        serde_json::to_value(&document).unwrap()
    );
}

#[test]
fn recording_wire_document_matches_persisted_recording_json() {
    let recording = catalog_recording(
        "wire-shape",
        "alpha",
        1_800_000_000,
        Some("main"),
        TranscriptionStatus::Complete,
    );
    let document = crate::runtime::wire::recording_document(recording.clone());
    assert_eq!(
        serde_json::to_value(&recording).unwrap(),
        serde_json::to_value(&document).unwrap()
    );
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
        &Response::Settings(SettingsDocument::default())
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
fn merged_video_cleanup_searches_all_linked_projects_and_rejects_busy_state() {
    let mut runtime = runtime(
        StartMode::Ready,
        StopMode::Ready,
        TranscriptionMode::Ready,
        false,
    );
    runtime.storage.projects = vec![
        catalog_project("demo", "Demo"),
        catalog_project("other", "Other"),
        ProjectFile {
            id: ProjectId::new(dicta_core::GENERAL_PROJECT_ID)
                .unwrap_or_else(|_| unreachable!("core General project ID is valid")),
            name: "General".to_owned(),
            created_at: Utc
                .timestamp_opt(1_800_000_000, 0)
                .single()
                .unwrap_or_else(|| unreachable!("static timestamp is valid")),
            source_path: None,
            extra: serde_json::Map::new(),
        },
    ];
    let cleanup = runtime.handle(request(ControlCommand::SettingsCleanupMerged {
        project: None,
    }));
    let Response::Cleanup(summary) = response(&cleanup) else {
        panic!("expected cleanup response");
    };
    assert_eq!(summary.removed_files, 4);
    assert_eq!(
        runtime
            .storage
            .cleanup_calls
            .iter()
            .map(ProjectId::as_str)
            .collect::<Vec<_>>(),
        ["demo", "other"]
    );
    runtime.storage.cleanup_calls.clear();
    runtime.handle(request(ControlCommand::SettingsCleanupMerged {
        project: Some("demo".to_owned()),
    }));
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
        TimelineNoteDocument {
            id: "later".to_owned(),
            timestamp_seconds: 18.5,
            text: "Later note".to_owned(),
            created_at,
            source: "typed".to_owned(),
            extra: serde_json::Map::new(),
        },
        TimelineNoteDocument {
            id: "earlier".to_owned(),
            timestamp_seconds: 3.0,
            text: "Earlier note".to_owned(),
            created_at,
            source: "voice".to_owned(),
            extra: serde_json::Map::new(),
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
        serde_json::to_value(&runtime.storage.recordings[0].timeline_notes).unwrap(),
        serde_json::to_value(&recording.timeline_notes).unwrap()
    );

    let before = runtime.storage.recordings[0].clone();
    let duplicate = TimelineNoteDocument {
        id: "duplicate".to_owned(),
        timestamp_seconds: 31.0,
        text: "Past the end".to_owned(),
        created_at,
        source: "typed".to_owned(),
        extra: serde_json::Map::new(),
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
