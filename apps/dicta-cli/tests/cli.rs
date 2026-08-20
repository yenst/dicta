use dicta_cli::{
    execute, offline::FileOfflineStore, parse, ClientFailure, ControlClient, FailureKind, Host,
    Invocation, Mode, Runtime, HELP,
};
use dicta_control::{
    cli::OutputFormat, Command, ExitCode, ModelInstallStage, ModelState, ModelStatusSummary,
    RecordingSelector, Response,
};
use std::{
    cell::{Cell, RefCell},
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct FakeControl {
    unavailable_probes: Cell<usize>,
    probes: Cell<usize>,
    response: Response,
    commands: RefCell<Vec<Command>>,
}

impl FakeControl {
    fn ready(response: Response) -> Self {
        Self {
            unavailable_probes: Cell::new(0),
            probes: Cell::new(0),
            response,
            commands: RefCell::new(Vec::new()),
        }
    }

    fn unavailable_then(count: usize, response: Response) -> Self {
        Self {
            unavailable_probes: Cell::new(count),
            probes: Cell::new(0),
            response,
            commands: RefCell::new(Vec::new()),
        }
    }
}

impl ControlClient for FakeControl {
    fn probe(&self, _socket: &Path) -> Result<(), ClientFailure> {
        let probe = self.probes.get();
        self.probes.set(probe + 1);
        if probe < self.unavailable_probes.get() {
            Err(ClientFailure::new(
                FailureKind::Unavailable,
                "socket absent",
            ))
        } else {
            Ok(())
        }
    }

    fn request(&self, _socket: &Path, command: Command) -> Result<Response, ClientFailure> {
        self.commands.borrow_mut().push(command);
        Ok(self.response.clone())
    }
}

#[derive(Default)]
struct FakeHost {
    available: Cell<bool>,
    launches: RefCell<Vec<(String, PathBuf)>>,
    copies: RefCell<Vec<String>>,
    sleeps: Cell<usize>,
}

impl Host for FakeHost {
    fn executable_available(&self, _executable: &OsStr) -> bool {
        self.available.get()
    }

    fn launch_background(&self, executable: &OsStr, socket: &Path) -> Result<(), ClientFailure> {
        self.launches.borrow_mut().push((
            executable.to_string_lossy().into_owned(),
            socket.to_path_buf(),
        ));
        Ok(())
    }

    fn copy_text(&self, text: &str) -> Result<(), ClientFailure> {
        self.copies.borrow_mut().push(text.to_string());
        Ok(())
    }

    fn sleep(&self, _duration: Duration) {
        self.sleeps.set(self.sleeps.get() + 1);
    }
}

fn invocation(arguments: &[&str]) -> Invocation {
    let mut values = arguments.to_vec();
    values.extend(["--socket", "/tmp/dicta-cli-test.sock"]);
    parse(values).unwrap()
}

#[test]
fn parses_global_mode_before_command_grammar() {
    let parsed = invocation(&[
        "context",
        "latest",
        "--copy",
        "--json",
        "--no-start",
        "--native-bin",
        "/opt/dicta native",
    ]);
    assert!(!parsed.auto_start);
    assert_eq!(parsed.native_binary, "/opt/dicta native");
    assert!(matches!(
        parsed.mode,
        Mode::Online(dicta_control::cli::CliInvocation {
            command: Command::Context {
                recording: RecordingSelector::Latest,
                copy: true,
                ..
            },
            output: OutputFormat::Json,
        })
    ));
}

#[test]
fn stable_help_is_available_without_a_daemon() {
    let parsed = invocation(&["--help"]);
    let control = FakeControl::ready(Response::Accepted);
    let host = FakeHost::default();
    let mut output = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: None,
        },
        &mut output,
        &mut Vec::new(),
    );
    assert_eq!(code, ExitCode::Success);
    assert_eq!(String::from_utf8(output).unwrap(), HELP);
    assert_eq!(control.probes.get(), 0);
}

#[test]
fn no_start_reports_socket_failure_with_stable_exit_code() {
    let parsed = invocation(&["status", "--no-start", "--json"]);
    let control = FakeControl::unavailable_then(usize::MAX, Response::Accepted);
    let host = FakeHost::default();
    let mut diagnostics = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: None,
        },
        &mut Vec::new(),
        &mut diagnostics,
    );
    assert_eq!(code, ExitCode::Unavailable);
    assert!(host.launches.borrow().is_empty());
    let json: serde_json::Value = serde_json::from_slice(&diagnostics).unwrap();
    assert_eq!(json["error"]["code"], "unavailable");
}

#[test]
fn launches_native_without_a_shell_then_waits_and_sends() {
    let parsed = invocation(&[
        "record",
        "toggle",
        "--native-bin",
        "/opt/Dicta Native; touch nope",
    ]);
    let control = FakeControl::unavailable_then(3, Response::Accepted);
    let host = FakeHost::default();
    host.available.set(true);
    let mut output = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: None,
        },
        &mut output,
        &mut Vec::new(),
    );
    assert_eq!(code, ExitCode::Success);
    assert_eq!(
        host.launches.borrow().as_slice(),
        [(
            "/opt/Dicta Native; touch nope".to_owned(),
            PathBuf::from("/tmp/dicta-cli-test.sock")
        )]
    );
    assert_eq!(host.sleeps.get(), 2);
    assert_eq!(
        control.commands.borrow().as_slice(),
        [Command::RecordToggle]
    );
    assert_eq!(output, b"ok\n");
}

#[test]
fn ui_auto_starts_one_background_host_then_sends_typed_show_request() {
    let parsed = invocation(&["ui"]);
    let control = FakeControl::unavailable_then(2, Response::Accepted);
    let host = FakeHost::default();
    host.available.set(true);
    let mut output = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: None,
        },
        &mut output,
        &mut Vec::new(),
    );
    assert_eq!(code, ExitCode::Success);
    assert_eq!(host.launches.borrow().len(), 1);
    assert_eq!(control.commands.borrow().as_slice(), [Command::UiShow]);
    assert_eq!(output, b"ok\n");
}

#[test]
fn context_copy_stays_in_the_client() {
    let parsed = invocation(&["context", "latest", "--copy"]);
    let control = FakeControl::ready(Response::Context {
        text: "captured context".to_string(),
    });
    let host = FakeHost::default();
    let mut output = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: None,
        },
        &mut output,
        &mut Vec::new(),
    );
    assert_eq!(code, ExitCode::Success);
    assert_eq!(host.copies.borrow().as_slice(), ["captured context"]);
    assert_eq!(
        control.commands.borrow().as_slice(),
        [Command::Context {
            recording: RecordingSelector::Latest,
            project: None,
            copy: false,
        }]
    );
    assert_eq!(output, b"captured context\n");
}

fn model_status_response() -> Response {
    Response::ModelStatus(ModelStatusSummary {
        active_model: Some("compact".to_owned()),
        active_model_path: Some("/models/compact.bin".to_owned()),
        quality_state: ModelState::Installing,
        quality_path: "/models/quality.bin".to_owned(),
        quality_size_bytes: 12,
        expected_download_bytes: 34,
        install_stage: Some(ModelInstallStage::Downloading),
        downloaded_bytes: Some(12),
        message: "downloading quality model".to_owned(),
        last_error: None,
    })
}

#[test]
fn model_status_has_stable_human_and_json_output() {
    for (arguments, json) in [
        (vec!["model", "status"], false),
        (vec!["model", "status", "--json"], true),
    ] {
        let parsed = invocation(&arguments);
        let control = FakeControl::ready(model_status_response());
        let host = FakeHost::default();
        let mut output = Vec::new();
        let code = execute(
            &parsed,
            &Runtime {
                control: &control,
                host: &host,
                offline: None,
            },
            &mut output,
            &mut Vec::new(),
        );
        assert_eq!(code, ExitCode::Success);
        if json {
            let status: serde_json::Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(status["type"], "model_status");
            assert_eq!(status["data"]["install_stage"], "downloading");
        } else {
            let status = String::from_utf8(output).unwrap();
            assert!(status.contains("quality: installing\n"));
            assert!(status.contains("install stage: downloading\n"));
            assert!(status.contains("downloaded: 12 bytes\n"));
        }
    }
}

#[test]
fn settings_have_stable_human_and_json_output() {
    let response = Response::Settings(dicta_core::storage::AppSettings {
        shortcut_id: "control_space".to_owned(),
        cleanup_merged_videos: false,
        branch_locking: true,
        transcription_language: "nl".to_owned(),
        general_path: Some("/data/general".to_owned()),
    });
    for (arguments, json) in [
        (vec!["settings", "get"], false),
        (vec!["settings", "get", "--json"], true),
    ] {
        let parsed = invocation(&arguments);
        let control = FakeControl::ready(response.clone());
        let host = FakeHost::default();
        let mut output = Vec::new();
        let code = execute(
            &parsed,
            &Runtime {
                control: &control,
                host: &host,
                offline: None,
            },
            &mut output,
            &mut Vec::new(),
        );
        assert_eq!(code, ExitCode::Success);
        let output = String::from_utf8(output).unwrap();
        if json {
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(value["data"]["transcription_language"], "nl");
        } else {
            assert!(output.contains("shortcut: control_space"));
            assert!(output.contains("branch locking: on"));
            assert!(output.contains("general path: /data/general"));
        }
    }
}

#[test]
fn online_recording_show_renders_shared_core_details() {
    let recording: dicta_core::RecordingFile = serde_json::from_value(serde_json::json!({
        "id": "recording-details",
        "project_id": "demo",
        "duration_seconds": 12.5,
        "note": "Explain the native catalog",
        "success": true,
        "transcript": "The persisted transcript.",
        "transcription_status": "complete"
    }))
    .unwrap();
    let parsed = invocation(&["recording", "show", "recording-details"]);
    let control = FakeControl::ready(Response::RecordingDetails(Box::new(recording)));
    let host = FakeHost::default();
    let mut output = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: None,
        },
        &mut output,
        &mut Vec::new(),
    );
    assert_eq!(code, ExitCode::Success);
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("id: recording-details"));
    assert!(output.contains("note: Explain the native catalog"));
    assert!(output.contains("The persisted transcript."));
}

#[test]
fn binary_help_is_the_public_help_contract() {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_dicta"))
        .arg("--help")
        .env("DICTA_SOCKET", "/tmp/not-contacted.sock")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), HELP);
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_version_matches_the_packaged_product_release() {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_dicta"))
        .arg("--version")
        .env("DICTA_SOCKET", "/tmp/not-contacted.sock")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "dicta 0.8.0\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn binary_parse_failures_honor_json_output() {
    let output = ProcessCommand::new(env!("CARGO_BIN_EXE_dicta"))
        .args(["--json", "not-a-command"])
        .env("DICTA_SOCKET", "/tmp/not-contacted.sock")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(ExitCode::Usage.get().into()));
    assert!(output.stdout.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(diagnostic["error"]["code"], "usage");
}

fn offline_fixture() -> std::path::PathBuf {
    let count = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("dicta-cli-offline-{}-{count}", std::process::id()));
    let project = root.join("demo");
    let day = project.join("recordings/2026-08-20");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        project.join("project.json"),
        r#"{"id":"demo","name":"Demo"}"#,
    )
    .unwrap();
    std::fs::write(
        day.join("older.json"),
        r#"{
          "id":"older",
          "project_id":"demo",
          "started_at":"2026-08-20T08:00:00Z",
          "duration_seconds":12.5,
          "git_branch":"main",
          "transcription_status":"pending"
        }"#,
    )
    .unwrap();
    std::fs::write(
        day.join("newer.json"),
        r#"{
          "id":"newer",
          "project_id":"demo",
          "started_at":"2026-08-20T10:00:00Z",
          "duration_seconds":42.25,
          "git_branch":"main",
          "note":"Explain the native overlay",
          "transcript_path":"newer.transcript.md",
          "transcription_status":"complete"
        }"#,
    )
    .unwrap();
    std::fs::write(
        day.join("newer.transcript.md"),
        "The overlay should stay transparent.",
    )
    .unwrap();
    std::fs::write(day.join("malformed.json"), "{broken").unwrap();
    root
}

fn run_offline(arguments: &[&str], root: &Path) -> (ExitCode, Vec<u8>, Vec<u8>, FakeHost) {
    let parsed = invocation(arguments);
    let control = FakeControl::unavailable_then(usize::MAX, Response::Accepted);
    let host = FakeHost::default();
    let offline = FileOfflineStore::at(root, root);
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let code = execute(
        &parsed,
        &Runtime {
            control: &control,
            host: &host,
            offline: Some(&offline),
        },
        &mut output,
        &mut diagnostics,
    );
    (code, output, diagnostics, host)
}

#[test]
fn offline_list_is_json_and_warns_about_malformed_metadata() {
    let root = offline_fixture();
    let (code, output, diagnostics, host) =
        run_offline(&["recording", "list", "--project", "demo", "--json"], &root);
    assert_eq!(code, ExitCode::Success);
    assert!(host.launches.borrow().is_empty());
    let response: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(response["type"], "recordings");
    assert_eq!(response["data"][0]["id"], "newer");
    assert_eq!(response["data"][1]["id"], "older");
    let warning: serde_json::Value = serde_json::from_slice(&diagnostics).unwrap();
    assert!(warning["warnings"][0]
        .as_str()
        .unwrap()
        .contains("malformed.json"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_show_latest_loads_the_typed_file_and_transcript_sidecar() {
    let root = offline_fixture();
    let (code, output, _, host) = run_offline(&["recording", "show", "latest", "--json"], &root);
    assert_eq!(code, ExitCode::Success);
    assert!(host.launches.borrow().is_empty());
    let recording: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(recording["id"], "newer");
    assert_eq!(
        recording["transcript"],
        "The overlay should stay transparent."
    );
    assert!(recording["metadata_path"]
        .as_str()
        .unwrap()
        .ends_with("newer.json"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_context_can_copy_without_starting_the_native_app() {
    let root = offline_fixture();
    let (code, output, _, host) =
        run_offline(&["context", "latest", "--project", "demo", "--copy"], &root);
    assert_eq!(code, ExitCode::Success);
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("# Dicta recording: newer"));
    assert!(output.contains("The overlay should stay transparent."));
    assert_eq!(host.copies.borrow().as_slice(), [output]);
    assert!(host.launches.borrow().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_missing_recording_uses_not_found_exit_code() {
    let root = offline_fixture();
    let (code, output, diagnostics, host) = run_offline(
        &["recording", "show", "does-not-exist", "--no-start"],
        &root,
    );
    assert_eq!(code, ExitCode::NotFound);
    assert!(output.is_empty());
    assert!(String::from_utf8(diagnostics)
        .unwrap()
        .contains("not_found"));
    assert!(host.launches.borrow().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_same_id_in_two_projects_is_preserved_and_requires_disambiguation() {
    let root = offline_fixture();
    let other_day = root.join("other/recordings/2026-08-20");
    std::fs::create_dir_all(&other_day).unwrap();
    std::fs::write(
        root.join("other/project.json"),
        r#"{"id":"other","name":"Other"}"#,
    )
    .unwrap();
    std::fs::write(
        other_day.join("same-id.json"),
        r#"{
          "id":"newer",
          "project_id":"other",
          "started_at":"2026-08-20T11:00:00Z",
          "note":"Another project's recording"
        }"#,
    )
    .unwrap();

    let (list_code, list_output, _, _) = run_offline(&["recording", "list", "--json"], &root);
    assert_eq!(list_code, ExitCode::Success);
    let list: serde_json::Value = serde_json::from_slice(&list_output).unwrap();
    let same_id = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|recording| recording["id"] == "newer")
        .count();
    assert_eq!(same_id, 2, "cross-project recordings were deduplicated");

    let (show_code, _, show_diagnostic, _) = run_offline(&["recording", "show", "newer"], &root);
    assert_eq!(show_code, ExitCode::Conflict);
    assert!(String::from_utf8(show_diagnostic)
        .unwrap()
        .contains("ambiguous between projects `other` and `demo`"));

    let (context_code, context_output, _, _) =
        run_offline(&["context", "newer", "--project", "demo"], &root);
    assert_eq!(context_code, ExitCode::Success);
    assert!(String::from_utf8(context_output)
        .unwrap()
        .contains("Project: Demo (`demo`)"));
    std::fs::remove_dir_all(root).unwrap();
}
