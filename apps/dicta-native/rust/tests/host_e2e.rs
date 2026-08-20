use dicta_control::{
    protocol::AppPhase, socket::LocalClient, Command, RecordingSelector, Response,
};
use dicta_core::{storage::read_json, AnnotationFile, RecordingFile};
use dicta_native_bridge::{
    dicta_native_host_annotation_command, dicta_native_host_join, dicta_native_host_last_error,
    dicta_native_host_overlay_stroke, dicta_native_host_record_start,
    dicta_native_host_record_stop, dicta_native_host_recording_detail,
    dicta_native_host_request_stop, dicta_native_host_settings_set, dicta_native_host_start,
    dicta_native_host_state, dicta_native_host_timeline_notes_set, dicta_native_host_ui_snapshot,
    DictaNativeHostConfig, DictaNativeOverlayCommand, HOST_FLAG_E2E, RECORDING_DETAIL_MAX_BYTES,
    UI_SNAPSHOT_MAX_BYTES,
};
use std::{
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    slice,
    sync::Mutex,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
struct CallbackCommand {
    kind: u32,
    tool: u32,
    output: String,
}

unsafe extern "C" fn overlay_callback(
    context: *mut c_void,
    command: *const DictaNativeOverlayCommand,
) {
    if context.is_null() || command.is_null() {
        return;
    }
    // SAFETY: The test keeps both allocations alive until host join returns.
    let log = unsafe { &*context.cast::<Mutex<Vec<CallbackCommand>>>() };
    // SAFETY: The callback contract supplies a valid command for this invocation.
    let command = unsafe { &*command };
    let output = if command.output_name_len == 0 {
        String::new()
    } else {
        // SAFETY: The callback contract keeps this range alive for the call.
        let bytes = unsafe { slice::from_raw_parts(command.output_name, command.output_name_len) };
        String::from_utf8(bytes.to_vec()).expect("host output names are UTF-8")
    };
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(CallbackCommand {
            kind: command.kind,
            tool: command.tool,
            output,
        });
}

fn fixture_root() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dicta-native-host-e2e-{}-{unique}",
        std::process::id()
    ))
}

fn request(socket: &Path, command: Command) -> Response {
    LocalClient::connect(socket)
        .expect("host socket accepts a client")
        .request(command)
        .expect("host handles the command")
}

fn host_error() -> String {
    // SAFETY: Null/zero is the documented length-query form.
    let length = unsafe { dicta_native_host_last_error(std::ptr::null_mut(), 0) };
    let mut bytes = vec![0_u8; length + 1];
    // SAFETY: `bytes` exposes the reported writable capacity.
    unsafe { dicta_native_host_last_error(bytes.as_mut_ptr(), bytes.len()) };
    String::from_utf8_lossy(&bytes[..length]).into_owned()
}

fn ui_snapshot() -> serde_json::Value {
    let mut bytes = vec![0_u8; UI_SNAPSHOT_MAX_BYTES];
    // SAFETY: The vector exposes its entire writable allocation for this call.
    let length = unsafe { dicta_native_host_ui_snapshot(bytes.as_mut_ptr(), bytes.len()) };
    assert!(length > 0, "UI snapshot failed: {}", host_error());
    serde_json::from_slice(&bytes[..length]).expect("host returns a valid UI snapshot")
}

fn assert_saved_recording(storage: &Path, note: &str) {
    let sidecar = storage.join("e2e/e2e-000001.annotations.json");
    let annotations: AnnotationFile = read_json(&sidecar).expect("annotation sidecar is persisted");
    assert_eq!(annotations.events.len(), 1);
    assert_eq!(annotations.events[0].points.len(), 2);
    let recording: RecordingFile =
        read_json(&storage.join("e2e/e2e-000001.json")).expect("recording metadata is persisted");
    assert_eq!(recording.note, note);
}

fn select_e2e_project(socket: &Path) {
    let Response::Status(status) = request(socket, Command::Status) else {
        panic!("status returned another response type");
    };
    assert_eq!(status.phase, AppPhase::Idle);
    let Response::Projects(projects) = request(socket, Command::ProjectList) else {
        panic!("project list returned another response type");
    };
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, "e2e");
    assert!(!projects[0].selected);
    assert_eq!(
        request(
            socket,
            Command::ProjectSelect {
                project: "e2e".to_owned(),
            },
        ),
        Response::Accepted
    );
    let Response::Project(Some(project)) = request(socket, Command::ProjectCurrent) else {
        panic!("current project returned another response type");
    };
    assert_eq!(project.id, "e2e");
    assert!(project.selected);
}

fn assert_recording_catalog(socket: &Path, note: &str) {
    let Response::Recordings(recordings) = request(
        socket,
        Command::RecordingList {
            project: Some("e2e".to_owned()),
            branch: None,
            limit: None,
        },
    ) else {
        panic!("recording list returned another response type");
    };
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].id, "e2e-000001");
    assert_eq!(recordings[0].project.as_deref(), Some("e2e"));
    let Response::RecordingDetails(recording) = request(
        socket,
        Command::RecordingShow {
            recording: RecordingSelector::Latest,
        },
    ) else {
        panic!("recording show returned another response type");
    };
    assert_eq!(recording.id.as_str(), "e2e-000001");
    assert_eq!(recording.project_id.as_str(), "e2e");
    assert_eq!(recording.note, note);
}

fn open_latest_recording(socket: &Path) {
    assert_eq!(
        request(
            socket,
            Command::RecordingOpen {
                recording: RecordingSelector::Latest,
            },
        ),
        Response::Accepted
    );
}

fn delete_recording_through_live_host(socket: &Path, storage: &Path) {
    assert_eq!(
        request(
            socket,
            Command::RecordingDelete {
                recording: RecordingSelector::Latest,
            },
        ),
        Response::Accepted
    );
    let Response::Recordings(recordings) = request(
        socket,
        Command::RecordingList {
            project: Some("e2e".to_owned()),
            branch: None,
            limit: None,
        },
    ) else {
        panic!("recording list returned another response type");
    };
    assert!(recordings.is_empty());
    assert!(!storage.join("e2e/e2e-000001.json").exists());
    assert!(!storage.join("e2e/e2e-000001.annotations.json").exists());
}

fn recording_detail(recording_id: &str) -> serde_json::Value {
    let mut bytes = vec![0_u8; RECORDING_DETAIL_MAX_BYTES];
    // SAFETY: Both byte ranges remain valid for this synchronous call.
    let length = unsafe {
        dicta_native_host_recording_detail(
            recording_id.as_ptr(),
            recording_id.len(),
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    assert!(length > 0, "recording detail failed: {}", host_error());
    serde_json::from_slice(&bytes[..length]).expect("recording detail is valid JSON")
}

fn add_timeline_note_through_abi(storage: &Path) {
    let recording_id = "e2e-000001";
    let notes = serde_json::json!([{
        "id": "e2e-note-1",
        "timestamp_seconds": 0.0,
        "text": "Review the overlay transition",
        "created_at": "2026-08-20T20:18:00Z",
        "source": "typed"
    }]);
    let json = serde_json::to_vec(&notes).expect("timeline-note fixture encodes");
    // SAFETY: Both byte ranges remain readable for this synchronous call.
    assert_eq!(
        unsafe {
            dicta_native_host_timeline_notes_set(
                recording_id.as_ptr(),
                recording_id.len(),
                json.as_ptr(),
                json.len(),
            )
        },
        0,
        "timeline-note update failed: {}",
        host_error()
    );
    let detail = recording_detail(recording_id);
    assert_eq!(detail["recording"]["timeline_notes"], notes);
    let persisted: RecordingFile = read_json(&storage.join("e2e/e2e-000001.json"))
        .expect("timeline notes persist to recording metadata");
    assert_eq!(persisted.timeline_notes.len(), 1);
    assert_eq!(
        persisted.timeline_notes[0].text,
        "Review the overlay transition"
    );
}

fn exercise_annotation_abi() {
    assert_eq!(dicta_native_host_annotation_command(1, 0), 0);
    assert_eq!(dicta_native_host_annotation_command(3, 3), 0);
    let snapshot = ui_snapshot();
    assert_eq!(snapshot["status"]["annotations_enabled"], true);
    assert_eq!(snapshot["status"]["annotation_tool"], "spotlight");
    assert_eq!(dicta_native_host_annotation_command(4, 0), 0);
    assert_eq!(dicta_native_host_annotation_command(5, 0), 0);
    assert_eq!(dicta_native_host_annotation_command(3, 0), 0);
}

fn exercise_settings_abi(socket: &Path, storage: &Path) {
    // SAFETY: The language bytes remain readable for this synchronous call.
    assert_eq!(
        unsafe { dicta_native_host_settings_set(4, b"nl".as_ptr(), 2) },
        0
    );
    let Response::Settings(settings) = request(socket, Command::SettingsGet) else {
        panic!("settings get returned another response type");
    };
    assert_eq!(settings.transcription_language, "nl");
    assert_eq!(ui_snapshot()["settings"]["transcription_language"], "nl");
    let persisted: dicta_core::storage::AppSettings =
        read_json(&storage.join("settings.json")).expect("settings are persisted");
    assert_eq!(persisted.transcription_language, "nl");
}

fn wait_until_ready(socket: &Path) {
    for _ in 0..100 {
        if dicta_native_host_state() == 2 && socket.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        dicta_native_host_state(),
        2,
        "host did not become ready: {}",
        host_error()
    );
}

#[test]
fn one_process_host_serves_recording_lifecycle_and_overlay_callbacks() {
    let root = fixture_root();
    let socket = root.join("runtime/control.sock");
    let storage = root.join("storage");
    let output = "E2E-1";
    let callback_log = Box::new(Mutex::<Vec<CallbackCommand>>::new(Vec::new()));
    let callback_context = Box::into_raw(callback_log);
    let socket_text = socket.to_string_lossy();
    let storage_text = storage.to_string_lossy();
    let config = DictaNativeHostConfig {
        socket_path: socket_text.as_ptr(),
        socket_path_len: socket_text.len(),
        storage_root: storage_text.as_ptr(),
        storage_root_len: storage_text.len(),
        output_name: output.as_ptr(),
        output_name_len: output.len(),
        flags: HOST_FLAG_E2E,
    };

    // SAFETY: Config strings and callback context outlive host join below.
    assert_eq!(
        unsafe {
            dicta_native_host_start(
                std::ptr::from_ref(&config),
                Some(overlay_callback),
                callback_context.cast(),
            )
        },
        0
    );

    wait_until_ready(&socket);

    select_e2e_project(&socket);
    assert_eq!(request(&socket, Command::UiShow), Response::Accepted);
    exercise_settings_abi(&socket, &storage);
    let note = "native E2E";
    // SAFETY: The note remains readable for the synchronous FFI call.
    assert_eq!(
        unsafe { dicta_native_host_record_start(note.as_ptr(), note.len()) },
        0
    );
    let Response::Status(status) = request(&socket, Command::Status) else {
        panic!("status returned another response type");
    };
    assert_eq!(status.phase, AppPhase::Recording);
    let snapshot = ui_snapshot();
    assert_eq!(snapshot["version"], 1);
    assert_eq!(snapshot["status"]["phase"], "recording");
    assert_eq!(snapshot["recordings"], serde_json::json!([]));
    exercise_annotation_abi();
    let points = [0.1_f64, 0.2, 0.8, 0.9];
    // SAFETY: The point array remains readable for the full synchronous call.
    assert_eq!(
        unsafe { dicta_native_host_overlay_stroke(0, 0.01, 0.02, points.as_ptr(), 2) },
        0
    );
    assert_eq!(dicta_native_host_annotation_command(2, 0), 0);
    assert_eq!(dicta_native_host_record_stop(), 0);
    let Response::Status(status) = request(&socket, Command::Status) else {
        panic!("status returned another response type");
    };
    assert_eq!(status.phase, AppPhase::Idle);

    assert_saved_recording(&storage, note);
    let detail = recording_detail("e2e-000001");
    assert_eq!(detail["version"], 1);
    assert_eq!(detail["recording"]["note"], note);
    assert_eq!(detail["recording"]["id"], "e2e-000001");

    assert_recording_catalog(&socket, note);
    add_timeline_note_through_abi(&storage);
    open_latest_recording(&socket);

    let snapshot = ui_snapshot();
    assert_eq!(snapshot["status"]["phase"], "idle");
    assert_eq!(snapshot["recordings"][0]["id"], "e2e-000001");
    delete_recording_through_live_host(&socket, &storage);

    dicta_native_host_request_stop();
    assert_eq!(dicta_native_host_join(), 0);
    assert_eq!(dicta_native_host_state(), 0);
    assert!(!socket.exists(), "service left its socket behind");

    // SAFETY: The service thread is joined, so callbacks can no longer access it.
    let callback_log = unsafe { Box::from_raw(callback_context) };
    let commands = callback_log
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(commands
        .iter()
        .any(|command| { command.kind == 1 && command.output == output }));
    assert!(commands.iter().any(|command| command.kind == 2));
    assert!(commands
        .iter()
        .any(|command| command.kind == 3 && command.tool == 1));
    assert!(commands
        .iter()
        .any(|command| command.kind == 4 && command.tool == 3));
    assert!(commands.iter().any(|command| command.kind == 5));
    assert!(commands.iter().any(|command| command.kind == 6));
    assert!(commands.iter().any(|command| command.kind == 7));
    assert!(commands.iter().any(|command| command.kind == 8));
    assert!(commands
        .iter()
        .any(|command| command.kind == 9 && command.output == "e2e-000001"));
    drop(commands);

    fs::remove_dir_all(root).expect("E2E fixture cleans up");
}
