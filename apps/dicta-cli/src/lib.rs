#![forbid(unsafe_code)]

pub mod offline;

use dicta_control::{
    cli::{CliInvocation, OutputFormat},
    error::ExitCode,
    protocol::{
        AppPhase, ModelInstallStage, ModelState, ModelStatusSummary, RecordingDocument, Response,
        StatusSnapshot, TranscriptionState,
    },
    Command, Event, EventEnvelope, ServerMessage,
};
use serde_json::json;
use std::{
    env,
    ffi::{OsStr, OsString},
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    thread,
    time::Duration,
};

use offline::{OfflinePayload, OfflineRead, OfflineStore};

pub const HELP: &str = r#"Dicta native recording client

Usage: dicta [OPTIONS] <COMMAND>

Commands:
  ui                                  Show and raise the native dashboard
  status                              Show application status
  settings get                        Show persistent native settings
  settings shortcut ID                Set the recording shortcut preset
  settings language <auto|nl|en|fr|de|es>
  settings cleanup <on|off>
  settings cleanup-now [--project ID]  Search all linked Git projects unless ID is set
  settings branch-locking <on|off>
  settings general-path <PATH|default>
  model status                        Show local Whisper model state
  model install quality               Install verified high-quality Whisper model
  record start [--project ID] [--note TEXT]
  record stop | toggle | status
  project list | current | create NAME
  project add PATH [--name NAME]
  project select | refresh | remove PROJECT
  recording list [--project ID] [--branch NAME] [--limit N]
  recording show | open | transcribe | delete <ID|latest>
  context <ID|latest> [--project ID] [--copy]
  annotate toggle | enable | disable | undo | clear
  annotate tool <pen|arrow|rectangle|spotlight>
  events [--since SEQUENCE] [--follow]
  doctor                              Inspect local native integration

Options:
  --json              Emit one JSON value
  --socket PATH       Override DICTA_SOCKET; forwarded when auto-starting native
  --native-bin PATH   Override DICTA_NATIVE_BIN and dicta-native
  --no-start          Do not start dicta-native when the socket is absent
  -h, --help          Print help
  -V, --version       Print version
"#;

const START_ATTEMPTS: usize = 40;
const START_RETRY_DELAY: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Help,
    Version,
    Doctor(OutputFormat),
    Online(CliInvocation),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Invocation {
    pub mode: Mode,
    pub socket: PathBuf,
    pub native_binary: OsString,
    pub auto_start: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    Usage,
    NotFound,
    Unavailable,
    Software,
    PermissionDenied,
    Conflict,
}

impl FailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::NotFound => "not_found",
            Self::Unavailable => "unavailable",
            Self::Software => "software",
            Self::PermissionDenied => "permission_denied",
            Self::Conflict => "conflict",
        }
    }

    pub const fn exit_code(self) -> ExitCode {
        match self {
            Self::Usage => ExitCode::Usage,
            Self::NotFound => ExitCode::NotFound,
            Self::Unavailable => ExitCode::Unavailable,
            Self::Software => ExitCode::Software,
            Self::PermissionDenied => ExitCode::PermissionDenied,
            Self::Conflict => ExitCode::Conflict,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientFailure {
    pub kind: FailureKind,
    pub message: String,
}

impl ClientFailure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ClientFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ClientFailure {}

pub trait ControlClient {
    fn probe(&self, socket: &Path) -> Result<(), ClientFailure>;
    fn request(&self, socket: &Path, command: Command) -> Result<Response, ClientFailure>;
    fn stream_events(
        &self,
        socket: &Path,
        since_sequence: Option<u64>,
        follow: bool,
        emit: &mut dyn FnMut(EventEnvelope) -> Result<(), ClientFailure>,
    ) -> Result<(), ClientFailure> {
        let _ = (follow, emit);
        self.request(
            socket,
            Command::Events {
                since_sequence,
                follow: false,
            },
        )
        .map(drop)
    }
}

pub trait Host {
    fn executable_available(&self, executable: &OsStr) -> bool;
    fn launch_background(&self, executable: &OsStr, socket: &Path) -> Result<(), ClientFailure>;
    fn copy_text(&self, text: &str) -> Result<(), ClientFailure>;
    fn sleep(&self, duration: Duration);
}

pub struct Runtime<'a> {
    pub control: &'a dyn ControlClient,
    pub host: &'a dyn Host,
    pub offline: Option<&'a dyn OfflineStore>,
}

pub fn parse<I, S>(arguments: I) -> Result<Invocation, ClientFailure>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    let mut socket = env::var_os("DICTA_SOCKET").map(PathBuf::from);
    let mut native_binary = env::var_os("DICTA_NATIVE_BIN")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("dicta-native"));
    let mut auto_start = true;
    let mut json_output = false;
    let mut command = Vec::new();
    let mut position = 0;

    while position < args.len() {
        let value = args[position].to_str().ok_or_else(|| {
            ClientFailure::new(FailureKind::Usage, "arguments must be valid UTF-8")
        })?;
        match value {
            "--socket" | "--native-bin" => {
                let next = args.get(position + 1).ok_or_else(|| {
                    ClientFailure::new(FailureKind::Usage, format!("{value} requires a value"))
                })?;
                if value == "--socket" {
                    socket = Some(PathBuf::from(next));
                } else {
                    native_binary = next.clone();
                }
                position += 2;
            }
            "--no-start" => {
                auto_start = false;
                position += 1;
            }
            "--json" => {
                json_output = true;
                position += 1;
            }
            _ => {
                command.push(args.remove(position));
            }
        }
    }

    let socket = match socket {
        Some(path) => path,
        None => dicta_control::socket::default_socket_path().map_err(map_control_error)?,
    };
    let words = command
        .iter()
        .map(|argument| {
            argument.to_str().map(str::to_owned).ok_or_else(|| {
                ClientFailure::new(FailureKind::Usage, "command must be valid UTF-8")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let output = if json_output {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };
    let mode = match words.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => Mode::Help,
        [flag] if flag == "--version" || flag == "-V" => Mode::Version,
        [doctor] if doctor == "doctor" => Mode::Doctor(output),
        [] => {
            return Err(ClientFailure::new(
                FailureKind::Usage,
                "no command provided; run `dicta --help`",
            ))
        }
        _ => {
            let invocation = CliInvocation::parse(words)
                .map_err(|error| ClientFailure::new(FailureKind::Usage, error.to_string()))?;
            Mode::Online(CliInvocation {
                command: invocation.command,
                output,
            })
        }
    };

    Ok(Invocation {
        mode,
        socket,
        native_binary,
        auto_start,
    })
}

pub fn execute(
    invocation: &Invocation,
    runtime: &Runtime<'_>,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> ExitCode {
    let result = match &invocation.mode {
        Mode::Help => write_text(output, HELP),
        Mode::Version => {
            writeln!(output, "dicta {}", env!("CARGO_PKG_VERSION")).map_err(io_failure)
        }
        Mode::Doctor(format) => run_doctor(invocation, runtime, *format, output),
        Mode::Online(cli) => run_online(invocation, runtime, cli, output, diagnostics),
    };
    match result {
        Ok(()) => ExitCode::Success,
        Err(error) => {
            let format = match &invocation.mode {
                Mode::Doctor(format) => *format,
                Mode::Online(cli) => cli.output,
                Mode::Help | Mode::Version => OutputFormat::Human,
            };
            let _ = write_diagnostic(diagnostics, format, &error);
            error.kind.exit_code()
        }
    }
}

fn run_online(
    invocation: &Invocation,
    runtime: &Runtime<'_>,
    cli: &CliInvocation,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<(), ClientFailure> {
    if let Err(error) = runtime.control.probe(&invocation.socket) {
        if error.kind != FailureKind::Unavailable || !invocation.auto_start {
            if error.kind != FailureKind::Unavailable {
                return Err(error);
            }
            return try_offline(runtime, cli, output, diagnostics)?.ok_or(error);
        }
        if let Some(offline) = runtime.offline {
            if let Some(read) = offline.read(&cli.command)? {
                write_offline_read(runtime.host, cli, output, diagnostics, &read)?;
                return Ok(());
            }
        }
        if !runtime.host.executable_available(&invocation.native_binary) {
            return Err(ClientFailure::new(
                FailureKind::Unavailable,
                format!(
                    "native application `{}` was not found",
                    invocation.native_binary.to_string_lossy()
                ),
            ));
        }
        runtime
            .host
            .launch_background(&invocation.native_binary, &invocation.socket)?;
        let mut last_error = error;
        let mut ready = false;
        for _ in 0..START_ATTEMPTS {
            match runtime.control.probe(&invocation.socket) {
                Ok(()) => {
                    ready = true;
                    break;
                }
                Err(error) if error.kind == FailureKind::Unavailable => {
                    last_error = error;
                    runtime.host.sleep(START_RETRY_DELAY);
                }
                Err(error) => return Err(error),
            }
        }
        if !ready {
            return Err(ClientFailure::new(
                FailureKind::Unavailable,
                format!(
                    "dicta-native did not create {} within {} ms: {}",
                    invocation.socket.display(),
                    START_ATTEMPTS * START_RETRY_DELAY.as_millis() as usize,
                    last_error.message
                ),
            ));
        }
    }

    if let Command::Events {
        since_sequence,
        follow,
    } = cli.command
    {
        return runtime.control.stream_events(
            &invocation.socket,
            since_sequence,
            follow,
            &mut |event| write_event(output, cli.output, &event),
        );
    }

    let (command, should_copy) = without_server_copy(&cli.command);
    let response = runtime.control.request(&invocation.socket, command)?;
    if should_copy {
        match &response {
            Response::Context { text } => runtime.host.copy_text(text)?,
            _ => {
                return Err(ClientFailure::new(
                    FailureKind::Software,
                    "server returned a non-context response for `context --copy`",
                ))
            }
        }
    }
    write_response(output, cli.output, &response)
}

fn try_offline(
    runtime: &Runtime<'_>,
    cli: &CliInvocation,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Result<Option<()>, ClientFailure> {
    let Some(offline) = runtime.offline else {
        return Ok(None);
    };
    let Some(read) = offline.read(&cli.command)? else {
        return Ok(None);
    };
    write_offline_read(runtime.host, cli, output, diagnostics, &read)?;
    Ok(Some(()))
}

fn write_offline_read(
    host: &dyn Host,
    cli: &CliInvocation,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    read: &OfflineRead,
) -> Result<(), ClientFailure> {
    write_warnings(diagnostics, cli.output, &read.warnings)?;
    let OfflinePayload::Response(response) = &read.payload;
    if matches!(cli.command, Command::Context { copy: true, .. }) {
        if let Response::Context { text } = response {
            host.copy_text(text)?;
        }
    }
    write_response(output, cli.output, response)
}

fn write_warnings(
    diagnostics: &mut dyn Write,
    format: OutputFormat,
    warnings: &[String],
) -> Result<(), ClientFailure> {
    if warnings.is_empty() {
        return Ok(());
    }
    if format == OutputFormat::Json {
        serde_json::to_writer(&mut *diagnostics, &json!({ "warnings": warnings }))
            .map_err(json_failure)?;
        writeln!(diagnostics).map_err(io_failure)
    } else {
        for warning in warnings {
            writeln!(diagnostics, "dicta: warning: {warning}").map_err(io_failure)?;
        }
        Ok(())
    }
}

fn write_recording_file(
    output: &mut dyn Write,
    recording: &RecordingDocument,
) -> Result<(), ClientFailure> {
    writeln!(output, "id: {}", recording.id).map_err(io_failure)?;
    writeln!(output, "project: {}", recording.project_id).map_err(io_failure)?;
    if let Some(started) = recording.started_at {
        writeln!(output, "started: {started}").map_err(io_failure)?;
    }
    if let Some(duration) = recording.duration_seconds {
        writeln!(output, "duration: {duration:.3}s").map_err(io_failure)?;
    }
    if let Some(branch) = recording.git_branch.as_deref() {
        writeln!(output, "branch: {branch}").map_err(io_failure)?;
    }
    if !recording.note.trim().is_empty() {
        writeln!(output, "note: {}", recording.note.trim()).map_err(io_failure)?;
    }
    if let Some(transcript) = recording.transcript.as_deref() {
        writeln!(output, "\nTranscript:\n{}", transcript.trim()).map_err(io_failure)?;
    }
    Ok(())
}

fn without_server_copy(command: &Command) -> (Command, bool) {
    match command {
        Command::Context {
            recording,
            project,
            copy: true,
        } => (
            Command::Context {
                recording: recording.clone(),
                project: project.clone(),
                copy: false,
            },
            true,
        ),
        command => (command.clone(), false),
    }
}

fn run_doctor(
    invocation: &Invocation,
    runtime: &Runtime<'_>,
    format: OutputFormat,
    output: &mut dyn Write,
) -> Result<(), ClientFailure> {
    let socket = match runtime.control.probe(&invocation.socket) {
        Ok(()) => (true, "ready".to_string()),
        Err(error) => (false, error.message),
    };
    let native = runtime.host.executable_available(&invocation.native_binary);
    let clipboard = runtime.host.executable_available(OsStr::new("wl-copy"));
    if format == OutputFormat::Json {
        serde_json::to_writer(
            &mut *output,
            &json!({
                "socket": {
                    "path": invocation.socket,
                    "ready": socket.0,
                    "detail": socket.1,
                },
                "native": {
                    "executable": invocation.native_binary.to_string_lossy(),
                    "available": native,
                },
                "clipboard": { "executable": "wl-copy", "available": clipboard },
                "annotation_format_version": dicta_core::ANNOTATION_FORMAT_VERSION,
            }),
        )
        .map_err(json_failure)?;
        writeln!(output).map_err(io_failure)
    } else {
        writeln!(
            output,
            "socket: {} ({})",
            if socket.0 { "ready" } else { "not ready" },
            invocation.socket.display()
        )
        .map_err(io_failure)?;
        if !socket.0 {
            writeln!(output, "  {}", socket.1).map_err(io_failure)?;
        }
        writeln!(
            output,
            "native: {} ({})",
            if native { "available" } else { "missing" },
            invocation.native_binary.to_string_lossy()
        )
        .map_err(io_failure)?;
        writeln!(
            output,
            "clipboard: {} (wl-copy)",
            if clipboard { "available" } else { "missing" }
        )
        .map_err(io_failure)?;
        writeln!(
            output,
            "annotation format: v{}",
            dicta_core::ANNOTATION_FORMAT_VERSION
        )
        .map_err(io_failure)
    }
}

fn write_event(
    output: &mut dyn Write,
    format: OutputFormat,
    envelope: &EventEnvelope,
) -> Result<(), ClientFailure> {
    if format == OutputFormat::Json {
        serde_json::to_writer(&mut *output, envelope).map_err(json_failure)?;
        return writeln!(output).map_err(io_failure);
    }
    match &envelope.event {
        Event::UiShowRequested { sequence } => {
            writeln!(output, "{sequence} ui_show_requested").map_err(io_failure)
        }
        Event::UiRecordingRequested {
            sequence,
            recording_id,
        } => {
            writeln!(output, "{sequence} ui_recording_requested {recording_id}").map_err(io_failure)
        }
        Event::StateChanged { sequence, status } => writeln!(
            output,
            "{sequence} state_changed {}",
            phase_name(status.phase)
        )
        .map_err(io_failure),
        Event::RecordingStarted {
            sequence,
            recording_id,
        } => writeln!(output, "{sequence} recording_started {recording_id}").map_err(io_failure),
        Event::RecordingStopped {
            sequence,
            recording_id,
            duration_seconds,
        } => writeln!(
            output,
            "{sequence} recording_stopped {recording_id} {duration_seconds:.3}s"
        )
        .map_err(io_failure),
        Event::AnnotationCreated {
            sequence,
            tool,
            timestamp_seconds,
        } => writeln!(
            output,
            "{sequence} annotation_created {tool:?} {timestamp_seconds:.3}s"
        )
        .map_err(io_failure),
        Event::TranscriptionCompleted {
            sequence,
            recording_id,
        } => writeln!(output, "{sequence} transcription_completed {recording_id}")
            .map_err(io_failure),
        Event::Failed { sequence, error } => {
            writeln!(output, "{sequence} failed {}", error.message).map_err(io_failure)
        }
    }
}

fn write_response(
    output: &mut dyn Write,
    format: OutputFormat,
    response: &Response,
) -> Result<(), ClientFailure> {
    if format == OutputFormat::Json {
        serde_json::to_writer(&mut *output, response).map_err(json_failure)?;
        return writeln!(output).map_err(io_failure);
    }
    match response {
        Response::Accepted => writeln!(output, "ok").map_err(io_failure),
        Response::Settings(settings) => {
            writeln!(output, "shortcut: {}", settings.shortcut_id).map_err(io_failure)?;
            writeln!(
                output,
                "cleanup merged videos: {}",
                if settings.cleanup_merged_videos {
                    "on"
                } else {
                    "off"
                }
            )
            .map_err(io_failure)?;
            writeln!(
                output,
                "branch locking: {}",
                if settings.branch_locking { "on" } else { "off" }
            )
            .map_err(io_failure)?;
            writeln!(output, "language: {}", settings.transcription_language)
                .map_err(io_failure)?;
            writeln!(
                output,
                "general path: {}",
                settings.general_path.as_deref().unwrap_or("default")
            )
            .map_err(io_failure)
        }
        Response::Cleanup(summary) => {
            writeln!(output, "{}", summary.message).map_err(io_failure)?;
            writeln!(output, "removed files: {}", summary.removed_files).map_err(io_failure)?;
            writeln!(output, "freed bytes: {}", summary.freed_bytes).map_err(io_failure)?;
            if let Some(branch) = summary.default_branch.as_deref() {
                writeln!(output, "default branch: {branch}").map_err(io_failure)?;
            }
            if !summary.cleaned_branches.is_empty() {
                writeln!(
                    output,
                    "cleaned branches: {}",
                    summary.cleaned_branches.join(", ")
                )
                .map_err(io_failure)?;
            }
            Ok(())
        }
        Response::ModelInstallStarted => {
            writeln!(output, "High-quality model installation started").map_err(io_failure)
        }
        Response::Status(status) => write_status(output, status).map_err(io_failure),
        Response::ModelStatus(status) => write_model_status(output, status).map_err(io_failure),
        Response::Projects(projects) => {
            if projects.is_empty() {
                writeln!(output, "No projects").map_err(io_failure)?;
            }
            for project in projects {
                let marker = if project.selected { "*" } else { " " };
                writeln!(
                    output,
                    "{marker} {}\t{}",
                    project.name,
                    project.path.as_deref().unwrap_or(&project.id)
                )
                .map_err(io_failure)?;
            }
            Ok(())
        }
        Response::Project(Some(project)) => writeln!(
            output,
            "{}\t{}",
            project.name,
            project.path.as_deref().unwrap_or(&project.id)
        )
        .map_err(io_failure),
        Response::Project(None) => writeln!(output, "No project selected").map_err(io_failure),
        Response::Recordings(recordings) => {
            if recordings.is_empty() {
                writeln!(output, "No recordings").map_err(io_failure)?;
            }
            for recording in recordings {
                write_recording(output, recording).map_err(io_failure)?;
            }
            Ok(())
        }
        Response::Recording(recording) => write_recording(output, recording).map_err(io_failure),
        Response::RecordingDetails(recording) => write_recording_file(output, recording),
        Response::VoiceNote(status) => writeln!(
            output,
            "voice note: {:?} · {}",
            status.state, status.message
        )
        .map_err(io_failure),
        Response::Context { text } => write_text(output, text),
    }
}

fn write_status(output: &mut dyn Write, status: &StatusSnapshot) -> io::Result<()> {
    writeln!(output, "phase: {}", phase_name(status.phase))?;
    writeln!(
        output,
        "project: {}",
        status.project.as_deref().unwrap_or("none")
    )?;
    writeln!(
        output,
        "recording: {}",
        status.recording_id.as_deref().unwrap_or("none")
    )?;
    writeln!(
        output,
        "annotations: {}",
        if status.annotations_enabled {
            "on"
        } else {
            "off"
        }
    )
}

fn write_model_status(output: &mut dyn Write, status: &ModelStatusSummary) -> io::Result<()> {
    writeln!(
        output,
        "active: {}",
        status.active_model.as_deref().unwrap_or("none")
    )?;
    if let Some(path) = status.active_model_path.as_deref() {
        writeln!(output, "active path: {path}")?;
    }
    writeln!(
        output,
        "quality: {}",
        model_state_name(status.quality_state)
    )?;
    writeln!(output, "quality path: {}", status.quality_path)?;
    writeln!(
        output,
        "quality size: {} / {} bytes",
        status.quality_size_bytes, status.expected_download_bytes
    )?;
    if let Some(downloaded) = status.downloaded_bytes {
        writeln!(output, "downloaded: {downloaded} bytes")?;
    }
    if let Some(stage) = status.install_stage {
        writeln!(output, "install stage: {}", model_install_stage_name(stage))?;
    }
    writeln!(output, "message: {}", status.message)?;
    if let Some(error) = status.last_error.as_deref() {
        writeln!(output, "last error: {error}")?;
    }
    Ok(())
}

const fn model_state_name(state: ModelState) -> &'static str {
    match state {
        ModelState::Missing => "missing",
        ModelState::Partial => "partial",
        ModelState::Ready => "ready",
        ModelState::Invalid => "invalid",
        ModelState::Unverified => "unverified",
        ModelState::Installing => "installing",
    }
}

const fn model_install_stage_name(stage: ModelInstallStage) -> &'static str {
    match stage {
        ModelInstallStage::Locating => "locating",
        ModelInstallStage::Downloading => "downloading",
        ModelInstallStage::Verifying => "verifying",
        ModelInstallStage::Ready => "ready",
    }
}

fn write_recording(
    output: &mut dyn Write,
    recording: &dicta_control::RecordingSummary,
) -> io::Result<()> {
    writeln!(
        output,
        "{}\t{:.3}s\t{}\t{}",
        recording.id,
        recording.duration_seconds,
        recording.branch.as_deref().unwrap_or("-"),
        transcription_name(recording.transcription)
    )
}

fn phase_name(phase: AppPhase) -> &'static str {
    match phase {
        AppPhase::Idle => "idle",
        AppPhase::Preparing => "preparing",
        AppPhase::Recording => "recording",
        AppPhase::Stopping => "stopping",
        AppPhase::Transcribing => "transcribing",
        AppPhase::Failed => "failed",
    }
}

fn transcription_name(state: TranscriptionState) -> &'static str {
    match state {
        TranscriptionState::Pending => "pending",
        TranscriptionState::Processing => "processing",
        TranscriptionState::Complete => "complete",
        TranscriptionState::Failed => "failed",
        TranscriptionState::Unavailable => "unavailable",
    }
}

pub fn write_diagnostic(
    diagnostics: &mut dyn Write,
    format: OutputFormat,
    error: &ClientFailure,
) -> io::Result<()> {
    if format == OutputFormat::Json {
        serde_json::to_writer(
            &mut *diagnostics,
            &json!({ "error": { "code": error.kind.as_str(), "message": error.message } }),
        )?;
        writeln!(diagnostics)
    } else {
        writeln!(
            diagnostics,
            "dicta: {}: {}",
            error.kind.as_str(),
            error.message
        )
    }
}

fn write_text(output: &mut dyn Write, text: &str) -> Result<(), ClientFailure> {
    output.write_all(text.as_bytes()).map_err(io_failure)?;
    if !text.ends_with('\n') {
        output.write_all(b"\n").map_err(io_failure)?;
    }
    Ok(())
}

fn io_failure(error: io::Error) -> ClientFailure {
    ClientFailure::new(FailureKind::Software, format!("output failed: {error}"))
}

fn json_failure(error: serde_json::Error) -> ClientFailure {
    ClientFailure::new(
        FailureKind::Software,
        format!("JSON output failed: {error}"),
    )
}

fn map_control_error(error: dicta_control::socket::ControlError) -> ClientFailure {
    use dicta_control::socket::ControlError;
    match error {
        ControlError::Remote(error) => ClientFailure::new(
            match error.exit_code() {
                ExitCode::Usage => FailureKind::Usage,
                ExitCode::NotFound => FailureKind::NotFound,
                ExitCode::Unavailable => FailureKind::Unavailable,
                ExitCode::Software => FailureKind::Software,
                ExitCode::PermissionDenied => FailureKind::PermissionDenied,
                ExitCode::Conflict => FailureKind::Conflict,
                ExitCode::Success => FailureKind::Software,
            },
            error.to_string(),
        ),
        ControlError::Security(message) => {
            ClientFailure::new(FailureKind::PermissionDenied, message)
        }
        ControlError::Io(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::TimedOut
            ) =>
        {
            ClientFailure::new(FailureKind::Unavailable, error.to_string())
        }
        other => ClientFailure::new(FailureKind::Software, other.to_string()),
    }
}

#[derive(Default)]
pub struct SystemControl;

impl ControlClient for SystemControl {
    fn probe(&self, socket: &Path) -> Result<(), ClientFailure> {
        dicta_control::socket::LocalClient::connect(socket)
            .map(drop)
            .map_err(map_control_error)
    }

    fn request(&self, socket: &Path, command: Command) -> Result<Response, ClientFailure> {
        let mut client =
            dicta_control::socket::LocalClient::connect(socket).map_err(map_control_error)?;
        client.request(command).map_err(map_control_error)
    }

    fn stream_events(
        &self,
        socket: &Path,
        since_sequence: Option<u64>,
        follow: bool,
        emit: &mut dyn FnMut(EventEnvelope) -> Result<(), ClientFailure>,
    ) -> Result<(), ClientFailure> {
        let mut client =
            dicta_control::socket::LocalClient::connect(socket).map_err(map_control_error)?;
        if follow {
            client
                .send(Command::Events {
                    since_sequence,
                    follow: true,
                })
                .map_err(map_control_error)?;
            loop {
                match client.read_message().map_err(map_control_error)? {
                    ServerMessage::Event(event) => emit(event)?,
                    ServerMessage::Response(response) => match response.payload {
                        dicta_control::ResponsePayload::Success {
                            result: Response::Accepted,
                        } => {}
                        dicta_control::ResponsePayload::Success { result } => {
                            return Err(ClientFailure::new(
                                FailureKind::Software,
                                format!("event follow returned unexpected response: {result:?}"),
                            ))
                        }
                        dicta_control::ResponsePayload::Failure { error } => {
                            return Err(map_control_error(
                                dicta_control::socket::ControlError::Remote(error),
                            ))
                        }
                    },
                }
            }
        } else {
            client
                .request(Command::Events {
                    since_sequence,
                    follow: false,
                })
                .map_err(map_control_error)?;
            while let Some(event) = client.pop_event() {
                emit(event)?;
            }
            Ok(())
        }
    }
}

#[derive(Default)]
pub struct SystemHost;

impl Host for SystemHost {
    fn executable_available(&self, executable: &OsStr) -> bool {
        resolve_executable(executable).is_some()
    }

    fn launch_background(&self, executable: &OsStr, socket: &Path) -> Result<(), ClientFailure> {
        ProcessCommand::new(executable)
            .arg("--background")
            .arg("--socket")
            .arg(socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|error| {
                ClientFailure::new(
                    FailureKind::Unavailable,
                    format!(
                        "could not start `{} --background --socket {}`: {error}",
                        executable.to_string_lossy(),
                        socket.display()
                    ),
                )
            })
    }

    fn copy_text(&self, text: &str) -> Result<(), ClientFailure> {
        let mut child = ProcessCommand::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                ClientFailure::new(
                    FailureKind::Unavailable,
                    format!("could not start wl-copy: {error}"),
                )
            })?;
        child
            .stdin
            .take()
            .ok_or_else(|| ClientFailure::new(FailureKind::Software, "wl-copy stdin unavailable"))?
            .write_all(text.as_bytes())
            .map_err(|error| {
                ClientFailure::new(FailureKind::Software, format!("wl-copy failed: {error}"))
            })?;
        let result = child.wait_with_output().map_err(|error| {
            ClientFailure::new(FailureKind::Software, format!("wl-copy failed: {error}"))
        })?;
        if result.status.success() {
            Ok(())
        } else {
            Err(ClientFailure::new(
                FailureKind::Unavailable,
                format!(
                    "wl-copy exited with {}: {}",
                    result.status,
                    String::from_utf8_lossy(&result.stderr).trim()
                ),
            ))
        }
    }

    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

fn resolve_executable(executable: &OsStr) -> Option<PathBuf> {
    let candidate = PathBuf::from(executable);
    if candidate.components().count() > 1 {
        return is_executable_file(&candidate).then_some(candidate);
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(&candidate))
        .find(|path| is_executable_file(path))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
