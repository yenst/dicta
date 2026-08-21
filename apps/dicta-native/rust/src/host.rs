use crate::{DictaNativeOverlayCallback, DictaNativeOverlayCommand};
use dicta_capture::{
    AudioSelection, CaptureArtifact, CaptureBackend, CaptureOutput, Geometry,
    OutputTransform as CaptureTransform,
};
use dicta_control::{
    protocol::StatusSnapshot, socket::LocalClient, AnnotationTool, CleanupSummary, Command, Event,
    ModelStatusSummary, ProjectSummary, RecordingSelector, RecordingSummary, Response,
    VoiceNoteStatus,
};
use dicta_core::{
    storage::{annotation_sidecar_path, write_json_atomic, AppSettings},
    AnnotationFile, ProjectFile, ProjectId, RecordingFile, RecordingId, RecordingScope,
    TimelineNote, TranscriptionStatus,
};
use dicta_engine::RecordingSession;
use dicta_overlay::{AnnotationSession, InteractionMode, OutputTransform, SurfacePoint};
use dicta_runtime::{
    service::{LocalRuntimeService, ServiceConfig, ShutdownHandle},
    AnnotationPort, CapturePort, Clock, Completion, IdSource, PortError, PortErrorKind, Runtime,
    RuntimeConfig, StoragePort, TranscriptionPort,
};
use dicta_transcribe::TranscriptionOutput;
use serde_json::Map;
use std::{
    ffi::c_void,
    fs,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

pub const OVERLAY_SHOW: u32 = 1;
pub const OVERLAY_START_CLOCK: u32 = 2;
pub const OVERLAY_SET_ENABLED: u32 = 3;
pub const OVERLAY_SET_TOOL: u32 = 4;
pub const OVERLAY_UNDO: u32 = 5;
pub const OVERLAY_CLEAR: u32 = 6;
pub const OVERLAY_FINISH: u32 = 7;
pub const UI_SHOW_REQUESTED: u32 = 8;
pub const UI_OPEN_RECORDING_REQUESTED: u32 = 9;

#[derive(Clone, Copy)]
pub struct OverlayCallback {
    pub function: DictaNativeOverlayCallback,
    pub context: usize,
}

#[derive(Clone, Debug)]
pub struct HostConfig {
    pub socket_path: PathBuf,
    pub storage_root: PathBuf,
    pub output_name: String,
    pub e2e: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum HostState {
    Stopped = 0,
    Starting = 1,
    Running = 2,
    Stopping = 3,
    Failed = 4,
}

impl HostState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::Stopping,
            4 => Self::Failed,
            _ => Self::Stopped,
        }
    }
}

#[derive(Default)]
struct Diagnostics {
    state: AtomicU8,
    last_error: Mutex<String>,
    stroke_count: AtomicU64,
}

impl Diagnostics {
    fn set_state(&self, state: HostState) {
        self.state.store(state as u8, Ordering::Release);
    }

    fn state(&self) -> HostState {
        HostState::from_u8(self.state.load(Ordering::Acquire))
    }

    fn fail(&self, message: impl Into<String>) {
        *lock(&self.last_error) = message.into();
        self.set_state(HostState::Failed);
    }
}

struct ActiveHost {
    shutdown: ShutdownHandle,
    thread: JoinHandle<()>,
    diagnostics: Arc<Diagnostics>,
    annotations: Arc<Mutex<AnnotationRecorder>>,
    socket_path: PathBuf,
}

#[derive(Default)]
struct HostSlot {
    active: Option<ActiveHost>,
    joining: bool,
    last: Option<Arc<Diagnostics>>,
    detached_error: String,
}

static HOST: OnceLock<Mutex<HostSlot>> = OnceLock::new();

fn slot() -> &'static Mutex<HostSlot> {
    HOST.get_or_init(|| Mutex::new(HostSlot::default()))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone)]
struct OverlaySink {
    callback: Option<OverlayCallback>,
}

#[derive(Default)]
struct AnnotationRecorder {
    active: Option<ActiveAnnotations>,
}

struct ActiveAnnotations {
    recording_id: RecordingId,
    canvas: dicta_core::AnnotationCanvas,
    session: AnnotationSession,
}

impl AnnotationRecorder {
    fn start(
        &mut self,
        recording_id: RecordingId,
        output: &CaptureOutput,
    ) -> Result<(), PortError> {
        #[allow(clippy::cast_possible_truncation)]
        let scale = output.scale as f32;
        let canvas = dicta_core::AnnotationCanvas {
            output_name: Some(output.name.clone()),
            width_pixels: output.pixel_size.0,
            height_pixels: output.pixel_size.1,
            scale,
            extra: Map::new(),
        };
        let session = AnnotationSession::new(
            recording_id.clone(),
            canvas.clone(),
            overlay_transform(output.transform),
        )
        .map_err(annotation_error)?;
        self.active = Some(ActiveAnnotations {
            recording_id,
            canvas,
            session,
        });
        Ok(())
    }

    fn set_enabled(&mut self, recording_id: &RecordingId, enabled: bool) -> Result<(), PortError> {
        let active = self.require_active(recording_id)?;
        active
            .session
            .set_mode(if enabled {
                InteractionMode::Annotating
            } else {
                InteractionMode::PassThrough
            })
            .map_err(annotation_error)
    }

    fn undo(&mut self, recording_id: &RecordingId) -> Result<(), PortError> {
        self.require_active(recording_id)?
            .session
            .undo()
            .map(drop)
            .map_err(annotation_error)
    }

    fn clear(&mut self, recording_id: &RecordingId) -> Result<(), PortError> {
        self.require_active(recording_id)?
            .session
            .clear()
            .map(drop)
            .map_err(annotation_error)
    }

    fn append_stroke(
        &mut self,
        tool: u32,
        started_at_seconds: f64,
        ended_at_seconds: f64,
        points: &[f64],
    ) -> Result<(), PortError> {
        let active = self.active.as_mut().ok_or_else(|| {
            PortError::new(
                PortErrorKind::NotFound,
                "annotation stroke arrived without an active recording",
            )
        })?;
        let tool = annotation_tool(tool)?;
        let (width, height) = active.session.mapping().logical_size();
        let point_count = points.len() / 2;
        let started = duration_from_seconds(started_at_seconds)?;
        let ended = duration_from_seconds(ended_at_seconds)?;
        let first = surface_point(points[0], points[1], width, height);
        active
            .session
            .begin(tool, first, default_style(), started)
            .map_err(annotation_error)?;
        if point_count > 2 {
            for (index, point) in points
                .chunks_exact(2)
                .enumerate()
                .skip(1)
                .take(point_count - 2)
            {
                #[allow(clippy::cast_precision_loss)]
                let progress = index as f64 / (point_count - 1) as f64;
                let at = started + ended.saturating_sub(started).mul_f64(progress);
                active
                    .session
                    .update(surface_point(point[0], point[1], width, height), at)
                    .map_err(annotation_error)?;
            }
        }
        let last = points.chunks_exact(2).last().ok_or_else(|| {
            PortError::new(PortErrorKind::Internal, "annotation stroke has no points")
        })?;
        active
            .session
            .finish(surface_point(last[0], last[1], width, height), ended)
            .map(drop)
            .map_err(annotation_error)
    }

    fn finish(&mut self, recording_id: &RecordingId) -> Result<AnnotationFile, PortError> {
        let active = self.active.take().ok_or_else(|| {
            PortError::new(PortErrorKind::NotFound, "annotation session is not active")
        })?;
        if active.recording_id != *recording_id {
            self.active = Some(active);
            return Err(PortError::new(
                PortErrorKind::NotFound,
                "annotation recording ID does not match",
            ));
        }
        let mut file = AnnotationFile::new(active.recording_id, active.canvas);
        file.events = active.session.events().to_vec();
        Ok(file)
    }

    fn require_active(
        &mut self,
        recording_id: &RecordingId,
    ) -> Result<&mut ActiveAnnotations, PortError> {
        let active = self.active.as_mut().ok_or_else(|| {
            PortError::new(PortErrorKind::NotFound, "annotation session is not active")
        })?;
        if active.recording_id != *recording_id {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                "annotation recording ID does not match",
            ));
        }
        Ok(active)
    }
}

impl OverlaySink {
    fn send(&self, kind: u32, tool: u32, output_name: &str) {
        let Some(callback) = self.callback else {
            return;
        };
        let command = DictaNativeOverlayCommand {
            kind,
            tool,
            output_name: output_name.as_ptr(),
            output_name_len: output_name.len(),
        };
        // SAFETY: C++ guarantees that the context remains alive through host join.
        // The command and output bytes remain valid for this synchronous callback.
        unsafe {
            (callback.function)(
                callback.context as *mut c_void,
                std::ptr::from_ref(&command),
            );
        };
    }

    fn recording_started(&self, output_name: &str) {
        self.send(OVERLAY_SHOW, 0, output_name);
        self.send(OVERLAY_START_CLOCK, 0, "");
    }
}

pub fn start(config: HostConfig, callback: Option<OverlayCallback>) -> Result<(), String> {
    validate_config(&config)?;
    let socket_path = config.socket_path.clone();
    let mut host = lock(slot());
    if host.active.is_some() {
        return Err("Dicta native host is already active".to_owned());
    }
    if host.joining {
        return Err("Dicta native host is still joining".to_owned());
    }

    let diagnostics = Arc::new(Diagnostics::default());
    diagnostics.set_state(HostState::Starting);
    let annotations = Arc::new(Mutex::new(AnnotationRecorder::default()));
    let shutdown = ShutdownHandle::new();
    let thread_diagnostics = Arc::clone(&diagnostics);
    let thread_annotations = Arc::clone(&annotations);
    let thread_shutdown = shutdown.clone();
    let thread = thread::Builder::new()
        .name("dicta-service".to_owned())
        .spawn(move || {
            let sink = OverlaySink { callback };
            let result = if config.e2e {
                run_e2e(
                    config,
                    sink,
                    thread_annotations,
                    &thread_shutdown,
                    &thread_diagnostics,
                )
            } else {
                run_production(
                    config,
                    sink,
                    thread_annotations,
                    &thread_shutdown,
                    &thread_diagnostics,
                )
            };
            match result {
                Ok(()) if thread_diagnostics.state() != HostState::Failed => {
                    thread_diagnostics.set_state(HostState::Stopped);
                }
                Ok(()) => {}
                Err(message) => thread_diagnostics.fail(message),
            }
        })
        .map_err(|error| format!("could not spawn Dicta service thread: {error}"))?;

    host.last = Some(Arc::clone(&diagnostics));
    host.detached_error.clear();
    host.active = Some(ActiveHost {
        shutdown,
        thread,
        diagnostics,
        annotations,
        socket_path,
    });
    Ok(())
}

pub fn request_stop() {
    let host = lock(slot());
    if let Some(active) = &host.active {
        if active.diagnostics.state() != HostState::Failed {
            active.diagnostics.set_state(HostState::Stopping);
        }
        active.shutdown.request();
    }
}

pub fn join() -> Result<(), String> {
    request_stop();
    let active = {
        let mut host = lock(slot());
        if host.joining {
            return Err("Dicta native host join is already in progress".to_owned());
        }
        host.joining = true;
        host.active.take()
    };
    let Some(active) = active else {
        lock(slot()).joining = false;
        return Ok(());
    };
    let diagnostics = Arc::clone(&active.diagnostics);
    let result = active.thread.join().map_err(|_| {
        let message = "Dicta service thread panicked".to_owned();
        diagnostics.fail(message.clone());
        message
    });
    lock(slot()).joining = false;
    result
}

pub fn state() -> HostState {
    let host = lock(slot());
    host.active.as_ref().map_or_else(
        || {
            host.last
                .as_ref()
                .map_or(HostState::Stopped, |value| value.state())
        },
        |value| value.diagnostics.state(),
    )
}

pub fn stroke_count() -> u64 {
    diagnostics().map_or(0, |value| value.stroke_count.load(Ordering::Acquire))
}

pub fn last_error() -> String {
    let host = lock(slot());
    if let Some(diagnostics) = host
        .active
        .as_ref()
        .map(|value| &value.diagnostics)
        .or(host.last.as_ref())
    {
        let message = lock(&diagnostics.last_error).clone();
        if !message.is_empty() {
            return message;
        }
    }
    host.detached_error.clone()
}

#[derive(serde::Serialize)]
pub struct UiSnapshot {
    pub version: u16,
    pub status: StatusSnapshot,
    pub projects: Vec<ProjectSummary>,
    pub model: Option<ModelStatusSummary>,
    pub model_error: Option<String>,
    pub settings: AppSettings,
    pub recordings: Vec<RecordingSummary>,
}

pub fn start_recording(note: Option<String>) -> Result<(), String> {
    let response = control_request(Command::RecordStart {
        project: None,
        note,
    })?;
    expect_accepted(&response)
}

pub fn stop_recording() -> Result<(), String> {
    let response = control_request(Command::RecordStop)?;
    expect_accepted(&response)
}

pub fn annotation_command(action: u32, tool: u32) -> Result<(), String> {
    let command = match action {
        1 => Command::AnnotationEnable,
        2 => Command::AnnotationDisable,
        3 => Command::AnnotationTool {
            tool: match tool {
                0 => AnnotationTool::Pen,
                1 => AnnotationTool::Arrow,
                2 => AnnotationTool::Rectangle,
                3 => AnnotationTool::Spotlight,
                _ => return Err("annotation tool is invalid".to_owned()),
            },
        },
        4 => Command::AnnotationUndo,
        5 => Command::AnnotationClear,
        _ => return Err("annotation action is invalid".to_owned()),
    };
    let response = control_request(command)?;
    expect_accepted(&response)
}

pub fn settings_command(key: u32, value: String) -> Result<AppSettings, String> {
    let command = match key {
        1 => Command::SettingsSetShortcut { shortcut_id: value },
        2 => Command::SettingsSetCleanup {
            enabled: parse_setting_bool("cleanup", &value)?,
        },
        3 => Command::SettingsSetBranchLocking {
            enabled: parse_setting_bool("branch locking", &value)?,
        },
        4 => Command::SettingsSetLanguage { language: value },
        5 => Command::SettingsSetGeneralPath {
            path: (!value.trim().is_empty()).then_some(value),
        },
        _ => return Err("settings key is invalid".to_owned()),
    };
    match control_request(command)? {
        Response::Settings(settings) => Ok(settings),
        response => Err(format!(
            "settings update returned unexpected response: {response:?}"
        )),
    }
}

pub fn cleanup_merged(project_id: String) -> Result<CleanupSummary, String> {
    match control_request(Command::SettingsCleanupMerged {
        project: Some(project_id),
    })? {
        Response::Cleanup(summary) => Ok(summary),
        response => Err(format!(
            "merged-video cleanup returned unexpected response: {response:?}"
        )),
    }
}

pub fn install_quality_model() -> Result<(), String> {
    match control_request(Command::ModelInstall {
        model: dicta_control::ModelTier::Quality,
    })? {
        Response::ModelInstallStarted | Response::ModelStatus(_) => Ok(()),
        response => Err(format!(
            "model installation returned unexpected response: {response:?}"
        )),
    }
}

fn parse_setting_bool(label: &str, value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{label} setting must be true or false")),
    }
}

pub fn recording_detail(recording_id: String) -> Result<RecordingFile, String> {
    match control_request(Command::RecordingShow {
        recording: RecordingSelector::Id(recording_id),
    })? {
        Response::RecordingDetails(recording) => Ok(*recording),
        response => Err(format!(
            "recording detail returned unexpected response: {response:?}"
        )),
    }
}

pub fn delete_recording(recording_id: String) -> Result<(), String> {
    let response = control_request(Command::RecordingDelete {
        recording: RecordingSelector::Id(recording_id),
    })?;
    expect_accepted(&response)
}

pub fn transcribe_recording(recording_id: String) -> Result<(), String> {
    let response = control_request(Command::RecordingTranscribe {
        recording: RecordingSelector::Id(recording_id),
    })?;
    expect_accepted(&response)
}

pub fn set_timeline_notes(
    recording_id: String,
    notes: Vec<TimelineNote>,
) -> Result<RecordingFile, String> {
    match control_request(Command::RecordingSetTimelineNotes {
        recording: RecordingSelector::Id(recording_id),
        notes,
    })? {
        Response::RecordingDetails(recording) => Ok(*recording),
        response => Err(format!(
            "timeline-note update returned unexpected response: {response:?}"
        )),
    }
}

pub fn transcribe_voice_note(
    recording_id: String,
    note_id: String,
    timestamp_seconds: f64,
    audio_path: String,
) -> Result<VoiceNoteStatus, String> {
    match control_request(Command::RecordingVoiceNoteTranscribe {
        recording: RecordingSelector::Id(recording_id),
        note_id,
        timestamp_seconds,
        audio_path,
    })? {
        Response::VoiceNote(status) => Ok(status),
        response => Err(format!(
            "voice-note transcription returned unexpected response: {response:?}"
        )),
    }
}

pub fn voice_note_status() -> Result<VoiceNoteStatus, String> {
    match control_request(Command::RecordingVoiceNoteStatus)? {
        Response::VoiceNote(status) => Ok(status),
        response => Err(format!(
            "voice-note status returned unexpected response: {response:?}"
        )),
    }
}

pub fn cancel_voice_note() -> Result<VoiceNoteStatus, String> {
    match control_request(Command::RecordingVoiceNoteCancel)? {
        Response::VoiceNote(status) => Ok(status),
        response => Err(format!(
            "voice-note cancellation returned unexpected response: {response:?}"
        )),
    }
}

pub fn select_project(project_id: String) -> Result<(), String> {
    let response = control_request(Command::ProjectSelect {
        project: project_id,
    })?;
    expect_accepted(&response)
}

pub fn create_project(name: String) -> Result<(), String> {
    let response = control_request(Command::ProjectCreate { name })?;
    expect_accepted(&response)
}

pub fn recording_context(
    recording_id: String,
    project_id: Option<String>,
) -> Result<String, String> {
    match control_request(Command::Context {
        recording: RecordingSelector::Id(recording_id),
        project: project_id,
        copy: false,
    })? {
        Response::Context { text } => Ok(text),
        response => Err(format!(
            "recording context returned unexpected response: {response:?}"
        )),
    }
}

pub fn ui_snapshot() -> Result<UiSnapshot, String> {
    let status = match control_request(Command::Status)? {
        Response::Status(status) => status,
        response => return Err(format!("status returned unexpected response: {response:?}")),
    };
    let projects = match control_request(Command::ProjectList)? {
        Response::Projects(projects) => projects,
        response => {
            return Err(format!(
                "project list returned unexpected response: {response:?}"
            ))
        }
    };
    let (model, model_error) = match control_request(Command::ModelStatus) {
        Ok(Response::ModelStatus(model)) => (Some(model), None),
        Ok(response) => (
            None,
            Some(format!(
                "model status returned unexpected response: {response:?}"
            )),
        ),
        Err(error) => (None, Some(error)),
    };
    let settings = match control_request(Command::SettingsGet)? {
        Response::Settings(settings) => settings,
        response => {
            return Err(format!(
                "settings returned unexpected response: {response:?}"
            ))
        }
    };
    let recordings = match control_request(Command::RecordingList {
        project: status.project.clone(),
        branch: None,
        limit: Some(64),
    })? {
        Response::Recordings(recordings) => recordings,
        response => {
            return Err(format!(
                "recording list returned unexpected response: {response:?}"
            ))
        }
    };
    Ok(UiSnapshot {
        version: 1,
        status,
        projects,
        model,
        model_error,
        settings,
        recordings,
    })
}

fn control_request(command: Command) -> Result<Response, String> {
    let socket_path = {
        let host = lock(slot());
        host.active
            .as_ref()
            .map(|active| active.socket_path.clone())
            .ok_or_else(|| "Dicta native host is not running".to_owned())?
    };
    let mut client = LocalClient::connect(&socket_path).map_err(|error| error.to_string())?;
    client.request(command).map_err(|error| error.to_string())
}

fn expect_accepted(response: &Response) -> Result<(), String> {
    if *response == Response::Accepted {
        Ok(())
    } else {
        Err(format!(
            "command returned unexpected response: {response:?}"
        ))
    }
}

pub fn set_detached_error(message: String) {
    lock(slot()).detached_error = message;
}

pub fn record_stroke(
    tool: u32,
    started_at_seconds: f64,
    ended_at_seconds: f64,
    points: &[f64],
) -> Result<(), ()> {
    if tool > 3
        || !started_at_seconds.is_finite()
        || !ended_at_seconds.is_finite()
        || started_at_seconds < 0.0
        || ended_at_seconds < started_at_seconds
        || points.len() < 4
        || !points.len().is_multiple_of(2)
        || points
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(());
    }
    let (Some(diagnostics), Some(annotations)) = (diagnostics(), annotations()) else {
        return Err(());
    };
    lock(&annotations)
        .append_stroke(tool, started_at_seconds, ended_at_seconds, points)
        .map_err(|_| ())?;
    diagnostics.stroke_count.fetch_add(1, Ordering::AcqRel);
    Ok(())
}

fn diagnostics() -> Option<Arc<Diagnostics>> {
    let host = lock(slot());
    host.active
        .as_ref()
        .map(|value| Arc::clone(&value.diagnostics))
        .or_else(|| host.last.as_ref().map(Arc::clone))
}

fn annotations() -> Option<Arc<Mutex<AnnotationRecorder>>> {
    lock(slot())
        .active
        .as_ref()
        .map(|value| Arc::clone(&value.annotations))
}

fn validate_config(config: &HostConfig) -> Result<(), String> {
    if !config.socket_path.is_absolute() {
        return Err("native host socket path must be absolute".to_owned());
    }
    if !config.storage_root.is_absolute() {
        return Err("native host storage root must be absolute".to_owned());
    }
    if config.output_name.trim().is_empty() {
        return Err("native host output name must not be empty".to_owned());
    }
    Ok(())
}

fn service_config() -> ServiceConfig {
    ServiceConfig {
        max_requests_per_connection: NonZeroUsize::MIN,
        ..ServiceConfig::default()
    }
}

fn run_production(
    config: HostConfig,
    sink: OverlaySink,
    annotation_recorder: Arc<Mutex<AnnotationRecorder>>,
    shutdown: &ShutdownHandle,
    diagnostics: &Diagnostics,
) -> Result<(), String> {
    let annotations = QtAnnotations {
        sink: sink.clone(),
        recorder: Arc::clone(&annotation_recorder),
    };
    let observer_sink = sink.clone();
    let observer = move |session: &RecordingSession, output: &CaptureOutput| {
        lock(&annotation_recorder).start(session.recording_id.clone(), output)?;
        observer_sink.recording_started(&output.name);
        Ok(())
    };
    let mut linux_config = dicta_linux::LinuxConfig::new(config.storage_root, config.output_name);
    linux_config.audio = AudioSelection::Mixed {
        source_name: "dicta-default-mixed".to_owned(),
    };
    let runtime = dicta_linux::build_runtime_with_observer(linux_config, annotations, observer)
        .map_err(|error| error.to_string())?;
    run_service(config.socket_path, runtime, sink, shutdown, diagnostics)
}

fn run_e2e(
    config: HostConfig,
    sink: OverlaySink,
    annotation_recorder: Arc<Mutex<AnnotationRecorder>>,
    shutdown: &ShutdownHandle,
    diagnostics: &Diagnostics,
) -> Result<(), String> {
    let runtime = Runtime::new(
        E2eCapture::new(
            config.storage_root.clone(),
            config.output_name,
            sink.clone(),
            Arc::clone(&annotation_recorder),
        ),
        E2eTranscription,
        QtAnnotations {
            sink: sink.clone(),
            recorder: annotation_recorder,
        },
        E2eStorage::new(config.storage_root).with_project(),
        E2eClock,
        E2eIds { next: 1 },
        RuntimeConfig {
            transcribe_after_recording: false,
        },
    );
    run_service(config.socket_path, runtime, sink, shutdown, diagnostics)
}

fn run_service<C, T, A, S, K, I>(
    socket_path: PathBuf,
    runtime: Runtime<C, T, A, S, K, I>,
    sink: OverlaySink,
    shutdown: &ShutdownHandle,
    diagnostics: &Diagnostics,
) -> Result<(), String>
where
    C: CapturePort,
    T: TranscriptionPort,
    A: AnnotationPort,
    S: StoragePort,
    K: Clock,
    I: IdSource,
{
    let service = LocalRuntimeService::bind(socket_path, runtime, service_config())
        .map_err(|error| error.to_string())?;
    diagnostics.set_state(HostState::Running);
    service
        .run_until_shutdown_with_observer(shutdown, move |event| match &event.event {
            Event::UiShowRequested { .. } => sink.send(UI_SHOW_REQUESTED, 0, ""),
            Event::UiRecordingRequested { recording_id, .. } => {
                sink.send(UI_OPEN_RECORDING_REQUESTED, 0, recording_id);
            }
            _ => {}
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

struct QtAnnotations {
    sink: OverlaySink,
    recorder: Arc<Mutex<AnnotationRecorder>>,
}

impl AnnotationPort for QtAnnotations {
    fn set_enabled(&mut self, recording_id: &RecordingId, enabled: bool) -> Result<(), PortError> {
        lock(&self.recorder).set_enabled(recording_id, enabled)?;
        self.sink.send(OVERLAY_SET_ENABLED, u32::from(enabled), "");
        Ok(())
    }

    fn set_tool(
        &mut self,
        _recording_id: &RecordingId,
        tool: AnnotationTool,
    ) -> Result<(), PortError> {
        self.sink.send(OVERLAY_SET_TOOL, tool as u32, "");
        Ok(())
    }

    fn undo(&mut self, recording_id: &RecordingId) -> Result<(), PortError> {
        lock(&self.recorder).undo(recording_id)?;
        self.sink.send(OVERLAY_UNDO, 0, "");
        Ok(())
    }

    fn clear(&mut self, recording_id: &RecordingId) -> Result<(), PortError> {
        lock(&self.recorder).clear(recording_id)?;
        self.sink.send(OVERLAY_CLEAR, 0, "");
        Ok(())
    }

    fn finish(&mut self, recording_id: &RecordingId) -> Result<Option<AnnotationFile>, PortError> {
        self.sink.send(OVERLAY_FINISH, 0, "");
        lock(&self.recorder).finish(recording_id).map(Some)
    }
}

struct E2eCapture {
    root: PathBuf,
    output_name: String,
    sink: OverlaySink,
    recorder: Arc<Mutex<AnnotationRecorder>>,
    active: Option<(RecordingId, Instant, PathBuf)>,
}

impl E2eCapture {
    fn new(
        root: PathBuf,
        output_name: String,
        sink: OverlaySink,
        recorder: Arc<Mutex<AnnotationRecorder>>,
    ) -> Self {
        Self {
            root,
            output_name,
            sink,
            recorder,
            active: None,
        }
    }
}

impl CapturePort for E2eCapture {
    fn start(&mut self, session: &RecordingSession) -> Result<Completion<()>, PortError> {
        if self.active.is_some() {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "E2E capture is already active",
            ));
        }
        let path = self
            .root
            .join("e2e")
            .join(format!("{}.mp4", session.recording_id));
        let parent = path.parent().ok_or_else(|| {
            PortError::new(PortErrorKind::Internal, "E2E capture path has no parent")
        })?;
        fs::create_dir_all(parent).map_err(|error| e2e_io_error(&error))?;
        fs::write(&path, b"dicta-e2e-capture").map_err(|error| e2e_io_error(&error))?;
        let output = CaptureOutput {
            name: self.output_name.clone(),
            description: "Dicta E2E output".to_owned(),
            geometry: Geometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale: 1.0,
            pixel_size: (1920, 1080),
            transform: CaptureTransform::Normal,
            refresh_hz: 60.0,
            focused: true,
        };
        lock(&self.recorder).start(session.recording_id.clone(), &output)?;
        self.active = Some((session.recording_id.clone(), Instant::now(), path));
        self.sink.recording_started(&self.output_name);
        Ok(Completion::Ready(()))
    }

    fn stop(
        &mut self,
        session: &RecordingSession,
    ) -> Result<Completion<CaptureArtifact>, PortError> {
        let (recording_id, started, path) = self
            .active
            .take()
            .ok_or_else(|| PortError::new(PortErrorKind::NotFound, "E2E capture is not active"))?;
        if recording_id != session.recording_id {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                "E2E capture ID does not match",
            ));
        }
        Ok(Completion::Ready(CaptureArtifact {
            path,
            duration: started.elapsed(),
            backend: CaptureBackend::GpuScreenRecorder,
            output_name: self.output_name.clone(),
            geometry: Geometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale_milli: 1_000,
            encoded_pixel_size: (1920, 1080),
        }))
    }
}

struct E2eTranscription;

impl TranscriptionPort for E2eTranscription {
    fn transcribe(
        &mut self,
        _recording: &RecordingFile,
    ) -> Result<Completion<TranscriptionOutput>, PortError> {
        Ok(Completion::Pending)
    }
}

struct E2eStorage {
    root: PathBuf,
    projects: Vec<ProjectFile>,
    recordings: Vec<RecordingFile>,
    settings: AppSettings,
}

impl E2eStorage {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            projects: Vec::new(),
            recordings: Vec::new(),
            settings: AppSettings::default(),
        }
    }

    fn with_project(mut self) -> Self {
        self.projects.push(ProjectFile {
            id: ProjectId::new("e2e")
                .unwrap_or_else(|_| unreachable!("static E2E project ID is valid")),
            name: "E2E Project".to_owned(),
            created_at: std::time::UNIX_EPOCH.into(),
            source_path: Some(self.root.to_string_lossy().into_owned()),
        });
        self
    }
}

impl StoragePort for E2eStorage {
    fn load_settings(&mut self) -> Result<AppSettings, PortError> {
        Ok(self.settings.clone())
    }

    fn save_settings(&mut self, settings: &AppSettings) -> Result<(), PortError> {
        self.settings.clone_from(settings);
        write_json_atomic(&self.root.join("settings.json"), settings).map_err(|error| {
            PortError::new(
                PortErrorKind::Internal,
                format!("E2E settings failed: {error}"),
            )
        })
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
                "E2E recording was not found",
            ));
        };
        stored.timeline_notes = notes.to_vec();
        write_json_atomic(PathBuf::from(&stored.metadata_path).as_path(), stored).map_err(
            |error| {
                PortError::new(
                    PortErrorKind::Internal,
                    format!("E2E timeline-note save failed: {error}"),
                )
            },
        )?;
        Ok(stored.clone())
    }

    fn load_projects(&mut self) -> Result<Vec<ProjectFile>, PortError> {
        Ok(self.projects.clone())
    }

    fn load_recordings(&mut self) -> Result<Vec<RecordingFile>, PortError> {
        Ok(self.recordings.clone())
    }

    fn delete_recording(&mut self, recording: &RecordingFile) -> Result<(), PortError> {
        let Some(index) = self.recordings.iter().position(|candidate| {
            candidate.id == recording.id && candidate.project_id == recording.project_id
        }) else {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                "E2E recording was not found",
            ));
        };
        let located = &self.recordings[index];
        let mut artifacts = vec![
            PathBuf::from(&located.video_path),
            PathBuf::from(&located.metadata_path),
        ];
        artifacts.extend(located.annotation_path.as_deref().map(PathBuf::from));
        if artifacts.iter().any(|path| !path.starts_with(&self.root)) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "E2E recording escaped its storage root",
            ));
        }
        for artifact in artifacts {
            match fs::remove_file(artifact) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(PortError::new(
                        PortErrorKind::Internal,
                        format!("E2E artifact deletion failed: {error}"),
                    ));
                }
            }
        }
        self.recordings.remove(index);
        Ok(())
    }

    fn save_recording(
        &mut self,
        session: &RecordingSession,
        artifact: &CaptureArtifact,
        annotations: Option<&AnnotationFile>,
    ) -> Result<RecordingFile, PortError> {
        let metadata_path = self
            .root
            .join("e2e")
            .join(format!("{}.json", session.recording_id));
        let project_id = session.project_id.clone().unwrap_or_else(|| {
            ProjectId::new(dicta_core::GENERAL_PROJECT_ID)
                .unwrap_or_else(|_| unreachable!("core general project ID is valid"))
        });
        let annotation_path = annotations
            .map(|document| {
                let path = annotation_sidecar_path(&metadata_path);
                write_json_atomic(&path, document).map_err(|error| {
                    PortError::new(
                        PortErrorKind::Internal,
                        format!("E2E annotation sidecar failed: {error}"),
                    )
                })?;
                Ok::<String, PortError>(path.to_string_lossy().into_owned())
            })
            .transpose()?;
        let recording = RecordingFile {
            id: session.recording_id.clone(),
            project_id,
            video_path: artifact.path.to_string_lossy().into_owned(),
            metadata_path: metadata_path.to_string_lossy().into_owned(),
            note: session.note.clone().unwrap_or_default(),
            recording_scope: RecordingScope::Unprojected,
            git_branch: None,
            started_at: None,
            ended_at: None,
            duration_seconds: Some(artifact.duration.as_secs_f64()),
            size_bytes: fs::metadata(&artifact.path).ok().map(|value| value.len()),
            success: true,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: TranscriptionStatus::Unknown,
            transcription_error: None,
            transcription_language: None,
            poster_path: None,
            annotation_path,
            timeline_notes: Vec::new(),
            extra: Map::new(),
        };
        write_json_atomic(&metadata_path, &recording).map_err(|error| {
            PortError::new(
                PortErrorKind::Internal,
                format!("E2E metadata failed: {error}"),
            )
        })?;
        self.recordings.push(recording.clone());
        Ok(recording)
    }

    fn save_transcription(
        &mut self,
        _recording_id: &RecordingId,
        _output: &TranscriptionOutput,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

struct E2eClock;

impl Clock for E2eClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

struct E2eIds {
    next: u64,
}

impl IdSource for E2eIds {
    fn next_recording_id(&mut self, _now: SystemTime) -> Result<RecordingId, PortError> {
        let id = RecordingId::new(format!("e2e-{:06}", self.next)).map_err(|error| {
            PortError::new(PortErrorKind::Internal, format!("E2E ID failed: {error}"))
        })?;
        self.next = self.next.saturating_add(1);
        Ok(id)
    }
}

fn e2e_io_error(error: &std::io::Error) -> PortError {
    PortError::new(
        PortErrorKind::Internal,
        format!("E2E capture failed: {error}"),
    )
}

fn annotation_error(error: impl std::fmt::Display) -> PortError {
    PortError::new(PortErrorKind::Internal, error.to_string())
}

fn annotation_tool(tool: u32) -> Result<dicta_core::AnnotationTool, PortError> {
    match tool {
        0 => Ok(dicta_core::AnnotationTool::Pen),
        1 => Ok(dicta_core::AnnotationTool::Arrow),
        2 => Ok(dicta_core::AnnotationTool::Rectangle),
        3 => Ok(dicta_core::AnnotationTool::Spotlight),
        _ => Err(PortError::new(
            PortErrorKind::Internal,
            "annotation tool is invalid",
        )),
    }
}

fn duration_from_seconds(seconds: f64) -> Result<Duration, PortError> {
    Duration::try_from_secs_f64(seconds).map_err(annotation_error)
}

fn surface_point(x: f64, y: f64, width: f64, height: f64) -> SurfacePoint {
    SurfacePoint {
        x: x * width,
        y: y * height,
    }
}

fn default_style() -> dicta_core::AnnotationStyle {
    dicta_core::AnnotationStyle {
        color: "#ffcc00".to_owned(),
        width: 3.0,
        opacity: 1.0,
        extra: Map::new(),
    }
}

const fn overlay_transform(transform: CaptureTransform) -> OutputTransform {
    match transform {
        CaptureTransform::Normal => OutputTransform::Normal,
        CaptureTransform::Rotated90 => OutputTransform::Rotated90,
        CaptureTransform::Rotated180 => OutputTransform::Rotated180,
        CaptureTransform::Rotated270 => OutputTransform::Rotated270,
        CaptureTransform::Flipped => OutputTransform::Flipped,
        CaptureTransform::Flipped90 => OutputTransform::Flipped90,
        CaptureTransform::Flipped180 => OutputTransform::Flipped180,
        CaptureTransform::Flipped270 => OutputTransform::Flipped270,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_strokes() {
        assert!(record_stroke(4, 0.0, 1.0, &[0.0, 0.0]).is_err());
        assert!(record_stroke(0, 2.0, 1.0, &[0.0, 0.0]).is_err());
        assert!(record_stroke(0, 0.0, 1.0, &[1.5, 0.0]).is_err());
    }

    #[test]
    fn service_uses_one_request_per_connection() {
        assert_eq!(
            service_config().max_requests_per_connection,
            NonZeroUsize::MIN
        );
    }
}
