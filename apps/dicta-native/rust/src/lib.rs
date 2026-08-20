//! C ABI for the one-process Dicta native runtime host.

mod host;

use host::{HostConfig, OverlayCallback};
use std::{ffi::c_void, slice, str};

static API_VERSION: &[u8] = b"dicta-native/0.12\0";

pub const HOST_FLAG_E2E: u32 = 1;
pub const UI_SNAPSHOT_MAX_BYTES: usize = 64 * 1024;
pub const RECORDING_DETAIL_MAX_BYTES: usize = 1024 * 1024;
pub const RECORDING_CONTEXT_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const TIMELINE_NOTES_MAX_BYTES: usize = 1024 * 1024;
pub const CLEANUP_SUMMARY_MAX_BYTES: usize = 64 * 1024;

#[repr(C)]
pub struct DictaNativeHostConfig {
    pub socket_path: *const u8,
    pub socket_path_len: usize,
    pub storage_root: *const u8,
    pub storage_root_len: usize,
    pub output_name: *const u8,
    pub output_name_len: usize,
    pub flags: u32,
}

#[repr(C)]
pub struct DictaNativeOverlayCommand {
    pub kind: u32,
    pub tool: u32,
    pub output_name: *const u8,
    pub output_name_len: usize,
}

pub type DictaNativeOverlayCallback =
    unsafe extern "C" fn(context: *mut c_void, command: *const DictaNativeOverlayCommand);

/// Returns a process-lifetime C string describing the Rust bridge API.
#[no_mangle]
pub extern "C" fn dicta_native_api_version() -> *const std::ffi::c_char {
    API_VERSION.as_ptr().cast()
}

/// Counts Unicode scalar values in UTF-8 received from the Qt boundary.
///
/// # Safety
///
/// When `len` is nonzero, `data` must be non-null and point to a readable
/// allocation containing at least `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_utf8_scalar_count(data: *const u8, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    if data.is_null() {
        return 0;
    }

    // SAFETY: The caller contract requires a readable allocation of `len` bytes.
    let bytes = unsafe { slice::from_raw_parts(data, len) };
    str::from_utf8(bytes).map_or(0, |value| value.chars().count())
}

/// Starts the singleton native service thread.
///
/// Returns zero on success and a negative diagnostic code on failure.
///
/// # Safety
///
/// `config` must point to a valid configuration. Every non-empty byte range in
/// it must remain readable for this call. `callback_context` must remain valid
/// for callback invocations until [`dicta_native_host_join`] returns.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_start(
    config: *const DictaNativeHostConfig,
    callback: Option<DictaNativeOverlayCallback>,
    callback_context: *mut c_void,
) -> i32 {
    if config.is_null() {
        return -1;
    }
    // SAFETY: The null check and caller contract establish a valid config.
    let config = unsafe { &*config };
    // SAFETY: The same caller contract covers every referenced field.
    let parsed = unsafe { parse_config(config) };
    let config = match parsed {
        Ok(config) => config,
        Err(message) => {
            host::set_detached_error(message);
            return -2;
        }
    };
    let callback = callback.map(|function| OverlayCallback {
        function,
        context: callback_context as usize,
    });
    host::start(config, callback).map_or_else(
        |message| {
            host::set_detached_error(message);
            -3
        },
        |()| 0,
    )
}

#[no_mangle]
pub extern "C" fn dicta_native_host_request_stop() {
    host::request_stop();
}

/// Requests shutdown and joins the service thread.
#[no_mangle]
pub extern "C" fn dicta_native_host_join() -> i32 {
    host::join().map_or_else(
        |message| {
            host::set_detached_error(message);
            -1
        },
        |()| 0,
    )
}

#[no_mangle]
pub extern "C" fn dicta_native_host_state() -> u32 {
    host::state() as u32
}

#[no_mangle]
pub extern "C" fn dicta_native_host_stroke_count() -> u64 {
    host::stroke_count()
}

/// Starts an unprojected recording through the typed local control protocol.
///
/// # Safety
///
/// When `note_len` is nonzero, `note` must point to `note_len` readable bytes
/// containing UTF-8. Notes longer than 4096 bytes are rejected.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_record_start(note: *const u8, note_len: usize) -> i32 {
    if note_len > 4096 {
        host::set_detached_error("recording note exceeds 4096 UTF-8 bytes".to_owned());
        return -1;
    }
    // SAFETY: The function contract establishes the readable note range.
    let note = match unsafe { utf8_field(note, note_len, "recording note") } {
        Ok(note) => note,
        Err(message) => {
            host::set_detached_error(message);
            return -1;
        }
    };
    let note = (!note.trim().is_empty()).then_some(note);
    match host::start_recording(note) {
        Ok(()) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

/// Stops the active recording through the typed local control protocol.
#[no_mangle]
pub extern "C" fn dicta_native_host_record_stop() -> i32 {
    match host::stop_recording() {
        Ok(()) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

/// Applies one annotation command through the typed local control protocol.
///
/// Actions are versioned with this C ABI: 1 enable, 2 disable, 3 select tool,
/// 4 undo, and 5 clear. Tool values are 0 pen, 1 arrow, 2 rectangle, and
/// 3 spotlight; the tool field is ignored for other actions.
#[no_mangle]
pub extern "C" fn dicta_native_host_annotation_command(action: u32, tool: u32) -> i32 {
    match host::annotation_command(action, tool) {
        Ok(()) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

/// Updates one persistent native setting through the typed control protocol.
///
/// Keys are versioned with this C ABI: 1 shortcut, 2 cleanup policy, 3 branch
/// locking, 4 transcription language, and 5 General storage path. An empty
/// value resets the General path; boolean values are `true` or `false`.
///
/// # Safety
/// When `value_len` is nonzero, `value` must expose that many readable UTF-8
/// bytes. Values longer than 4096 bytes are rejected.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_settings_set(
    key: u32,
    value: *const u8,
    value_len: usize,
) -> i32 {
    if value_len > 4096 {
        host::set_detached_error("settings value exceeds 4096 UTF-8 bytes".to_owned());
        return -1;
    }
    // SAFETY: The function contract establishes the readable value range.
    let value = match unsafe { utf8_field(value, value_len, "settings value") } {
        Ok(value) => value,
        Err(message) => {
            host::set_detached_error(message);
            return -1;
        }
    };
    match host::settings_command(key, value) {
        Ok(_) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

/// Runs safe merged-branch video cleanup and writes its typed JSON summary.
///
/// # Safety
/// The project ID must be readable UTF-8 and `output` must expose `capacity`
/// writable bytes no larger than [`CLEANUP_SUMMARY_MAX_BYTES`].
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_cleanup_merged(
    project_id: *const u8,
    project_id_len: usize,
    output: *mut u8,
    capacity: usize,
) -> usize {
    if project_id_len == 0 || project_id_len > 256 {
        host::set_detached_error("cleanup project ID is invalid".to_owned());
        return 0;
    }
    // SAFETY: The function contract establishes the readable project ID range.
    let project_id = match unsafe { utf8_field(project_id, project_id_len, "project ID") } {
        Ok(value) => value,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    let summary = match host::cleanup_merged(project_id) {
        Ok(summary) => summary,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    // SAFETY: This function forwards the caller's writable-buffer contract.
    unsafe { write_json_payload(&summary, output, capacity, CLEANUP_SUMMARY_MAX_BYTES) }
}

/// Starts the nonblocking managed quality-model installation.
#[no_mangle]
pub extern "C" fn dicta_native_host_model_install_quality() -> i32 {
    match host::install_quality_model() {
        Ok(()) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -1
        }
    }
}

/// Writes a bounded JSON snapshot for the Qt recording dashboard.
///
/// The caller supplies one buffer of at most [`UI_SNAPSHOT_MAX_BYTES`]. Zero
/// indicates an unavailable snapshot; diagnostics are available through
/// [`dicta_native_host_last_error`].
///
/// # Safety
///
/// `output` must point to `capacity` writable bytes. Capacity must be nonzero
/// and no larger than [`UI_SNAPSHOT_MAX_BYTES`].
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_ui_snapshot(output: *mut u8, capacity: usize) -> usize {
    if output.is_null() || capacity == 0 || capacity > UI_SNAPSHOT_MAX_BYTES {
        host::set_detached_error("UI snapshot buffer is invalid".to_owned());
        return 0;
    }
    let snapshot = match host::ui_snapshot() {
        Ok(snapshot) => snapshot,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    // SAFETY: This function forwards the caller's writable-buffer contract.
    unsafe { write_json_payload(&snapshot, output, capacity, UI_SNAPSHOT_MAX_BYTES) }
}

/// Writes one recording detail payload as bounded, versioned JSON.
///
/// # Safety
///
/// The recording ID must be readable UTF-8 for `recording_id_len` bytes.
/// `output` must expose `capacity` writable bytes, with capacity no larger than
/// [`RECORDING_DETAIL_MAX_BYTES`].
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_recording_detail(
    recording_id: *const u8,
    recording_id_len: usize,
    output: *mut u8,
    capacity: usize,
) -> usize {
    if recording_id_len == 0 || recording_id_len > 256 {
        host::set_detached_error("recording ID length is invalid".to_owned());
        return 0;
    }
    // SAFETY: The function contract establishes the readable ID range.
    let recording_id = match unsafe { utf8_field(recording_id, recording_id_len, "recording ID") } {
        Ok(recording_id) => recording_id,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    let recording = match host::recording_detail(recording_id) {
        Ok(recording) => recording,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    let payload = serde_json::json!({"version": 1, "recording": recording});
    // SAFETY: This function forwards the caller's writable-buffer contract.
    unsafe { write_json_payload(&payload, output, capacity, RECORDING_DETAIL_MAX_BYTES) }
}

/// Deletes one persisted recording through the typed control protocol.
///
/// # Safety
/// `recording_id` must contain `recording_id_len` readable UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_recording_delete(
    recording_id: *const u8,
    recording_id_len: usize,
) -> i32 {
    // SAFETY: This function forwards the caller's readable-ID contract.
    unsafe { recording_action(recording_id, recording_id_len, host::delete_recording) }
}

/// Requests transcription for one persisted recording.
///
/// # Safety
/// `recording_id` must contain `recording_id_len` readable UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_recording_transcribe(
    recording_id: *const u8,
    recording_id_len: usize,
) -> i32 {
    // SAFETY: This function forwards the caller's readable-ID contract.
    unsafe { recording_action(recording_id, recording_id_len, host::transcribe_recording) }
}

/// Atomically replaces a recording's complete timeline-note collection.
///
/// # Safety
/// `recording_id` and `notes_json` must contain their declared number of
/// readable bytes. `notes_json` must encode an array of core `TimelineNote`
/// objects and may not exceed [`TIMELINE_NOTES_MAX_BYTES`].
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_timeline_notes_set(
    recording_id: *const u8,
    recording_id_len: usize,
    notes_json: *const u8,
    notes_json_len: usize,
) -> i32 {
    if recording_id_len == 0
        || recording_id_len > 256
        || recording_id.is_null()
        || notes_json_len == 0
        || notes_json_len > TIMELINE_NOTES_MAX_BYTES
        || notes_json.is_null()
    {
        host::set_detached_error("timeline-note request is invalid".to_owned());
        return -1;
    }
    // SAFETY: The function contract establishes both readable ranges.
    let recording_id = match unsafe { utf8_field(recording_id, recording_id_len, "recording ID") } {
        Ok(recording_id) => recording_id,
        Err(message) => {
            host::set_detached_error(message);
            return -1;
        }
    };
    // SAFETY: The function contract establishes the readable JSON range.
    let notes_bytes = unsafe { slice::from_raw_parts(notes_json, notes_json_len) };
    let notes = match serde_json::from_slice(notes_bytes) {
        Ok(notes) => notes,
        Err(error) => {
            host::set_detached_error(format!("timeline-note JSON is invalid: {error}"));
            return -1;
        }
    };
    match host::set_timeline_notes(recording_id, notes) {
        Ok(_) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

/// Selects the project used by subsequent recording and dashboard commands.
///
/// # Safety
/// `project_id` must contain `project_id_len` readable UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_project_select(
    project_id: *const u8,
    project_id_len: usize,
) -> i32 {
    // SAFETY: Project IDs and recording IDs share the same bounded UTF-8 ABI.
    unsafe { recording_action(project_id, project_id_len, host::select_project) }
}

/// Creates and selects a standalone project.
///
/// # Safety
/// `name` must contain `name_len` readable UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_project_create(name: *const u8, name_len: usize) -> i32 {
    if name_len == 0 || name_len > 256 || name.is_null() {
        host::set_detached_error("project name is invalid".to_owned());
        return -1;
    }
    // SAFETY: The caller contract establishes the readable name range.
    let name = match unsafe { utf8_field(name, name_len, "project name") } {
        Ok(value) => value,
        Err(message) => {
            host::set_detached_error(message);
            return -1;
        }
    };
    match host::create_project(name) {
        Ok(()) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

/// Writes the agent-ready context for one recording as UTF-8.
///
/// Passing an empty project ID leaves duplicate-ID detection unscoped.
///
/// # Safety
/// The ID ranges must be readable UTF-8 and `output` must expose `capacity`
/// writable bytes no larger than [`RECORDING_CONTEXT_MAX_BYTES`].
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_recording_context(
    recording_id: *const u8,
    recording_id_len: usize,
    project_id: *const u8,
    project_id_len: usize,
    output: *mut u8,
    capacity: usize,
) -> usize {
    if recording_id_len == 0 || recording_id_len > 256 {
        host::set_detached_error("recording ID length is invalid".to_owned());
        return 0;
    }
    // SAFETY: The caller contract establishes both readable ID ranges.
    let recording_id = match unsafe { utf8_field(recording_id, recording_id_len, "recording ID") } {
        Ok(value) => value,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    // SAFETY: The caller contract establishes both readable ID ranges.
    let project_id = match unsafe { utf8_field(project_id, project_id_len, "project ID") } {
        Ok(value) => (!value.trim().is_empty()).then_some(value),
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    let context = match host::recording_context(recording_id, project_id) {
        Ok(value) => value,
        Err(message) => {
            host::set_detached_error(message);
            return 0;
        }
    };
    // SAFETY: This function forwards the caller's writable-buffer contract.
    unsafe { write_utf8_payload(&context, output, capacity, RECORDING_CONTEXT_MAX_BYTES) }
}

unsafe fn recording_action(
    recording_id: *const u8,
    recording_id_len: usize,
    action: fn(String) -> Result<(), String>,
) -> i32 {
    if recording_id_len == 0 || recording_id_len > 256 || recording_id.is_null() {
        host::set_detached_error("recording ID is invalid".to_owned());
        return -1;
    }
    // SAFETY: The helper's caller establishes this readable range.
    let recording_id = match unsafe { utf8_field(recording_id, recording_id_len, "recording ID") } {
        Ok(recording_id) => recording_id,
        Err(message) => {
            host::set_detached_error(message);
            return -1;
        }
    };
    match action(recording_id) {
        Ok(()) => {
            host::set_detached_error(String::new());
            0
        }
        Err(message) => {
            host::set_detached_error(message);
            -2
        }
    }
}

unsafe fn write_json_payload(
    payload: &impl serde::Serialize,
    output: *mut u8,
    capacity: usize,
    maximum_capacity: usize,
) -> usize {
    if output.is_null() || capacity == 0 || capacity > maximum_capacity {
        host::set_detached_error("JSON payload buffer is invalid".to_owned());
        return 0;
    }
    let bytes = match serde_json::to_vec(payload) {
        Ok(bytes) => bytes,
        Err(error) => {
            host::set_detached_error(format!("could not encode JSON payload: {error}"));
            return 0;
        }
    };
    if bytes.len() > capacity {
        host::set_detached_error(format!(
            "JSON payload exceeds the {capacity}-byte caller buffer"
        ));
        return 0;
    }
    // SAFETY: The helper's caller provides `capacity` writable bytes and the
    // preceding check proves `bytes.len() <= capacity`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    host::set_detached_error(String::new());
    bytes.len()
}

unsafe fn write_utf8_payload(
    payload: &str,
    output: *mut u8,
    capacity: usize,
    maximum_capacity: usize,
) -> usize {
    if output.is_null() || capacity == 0 || capacity > maximum_capacity {
        host::set_detached_error("UTF-8 payload buffer is invalid".to_owned());
        return 0;
    }
    let bytes = payload.as_bytes();
    if bytes.len() > capacity {
        host::set_detached_error(format!(
            "UTF-8 payload exceeds the {capacity}-byte caller buffer"
        ));
        return 0;
    }
    // SAFETY: The helper's caller provides `capacity` writable bytes and the
    // preceding check proves `bytes.len() <= capacity`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    host::set_detached_error(String::new());
    bytes.len()
}

/// Copies the latest host error as UTF-8 and returns its full byte length.
///
/// Passing a null output or zero capacity performs a length query.
///
/// # Safety
///
/// A non-null `output` must be writable for `capacity` bytes.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_last_error(output: *mut u8, capacity: usize) -> usize {
    let message = host::last_error();
    let bytes = message.as_bytes();
    if !output.is_null() && capacity > 0 {
        let count = bytes.len().min(capacity.saturating_sub(1));
        // SAFETY: The caller contract requires `capacity` writable bytes.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, count) };
        // SAFETY: `count` is strictly less than nonzero `capacity`.
        unsafe { *output.add(count) = 0 };
    }
    bytes.len()
}

/// Receives a normalized stroke from the Qt scene-graph item.
///
/// # Safety
///
/// `xy` must contain `point_count * 2` readable `f64` values when non-null.
#[no_mangle]
pub unsafe extern "C" fn dicta_native_host_overlay_stroke(
    tool: u32,
    started_at_seconds: f64,
    ended_at_seconds: f64,
    xy: *const f64,
    point_count: usize,
) -> i32 {
    if point_count == 0 || xy.is_null() || point_count > 1_000_000 {
        return -1;
    }
    let Some(value_count) = point_count.checked_mul(2) else {
        return -1;
    };
    // SAFETY: The caller contract establishes the `point_count * 2` range.
    let points = unsafe { slice::from_raw_parts(xy, value_count) };
    match host::record_stroke(tool, started_at_seconds, ended_at_seconds, points) {
        Ok(()) => 0,
        Err(()) => -2,
    }
}

unsafe fn parse_config(config: &DictaNativeHostConfig) -> Result<HostConfig, String> {
    Ok(HostConfig {
        socket_path: unsafe {
            utf8_field(config.socket_path, config.socket_path_len, "socket path")?
        }
        .into(),
        storage_root: unsafe {
            utf8_field(config.storage_root, config.storage_root_len, "storage root")?
        }
        .into(),
        output_name: unsafe {
            utf8_field(config.output_name, config.output_name_len, "output name")?
        },
        e2e: config.flags & HOST_FLAG_E2E != 0,
    })
}

unsafe fn utf8_field(pointer: *const u8, len: usize, name: &str) -> Result<String, String> {
    if len == 0 {
        return Ok(String::new());
    }
    if pointer.is_null() {
        return Err(format!("{name} pointer is null"));
    }
    // SAFETY: The caller contract requires this field to be readable for `len` bytes.
    let bytes = unsafe { slice::from_raw_parts(pointer, len) };
    str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|error| format!("{name} is not UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_bridge_version() {
        let version = unsafe { std::ffi::CStr::from_ptr(dicta_native_api_version()) };
        assert_eq!(version.to_bytes(), b"dicta-native/0.12");
    }

    #[test]
    fn counts_unicode_scalars() {
        let value = "Dicta ✍️";
        let count = unsafe { dicta_native_utf8_scalar_count(value.as_ptr(), value.len()) };
        assert_eq!(count, value.chars().count());
    }

    #[test]
    fn rejects_a_null_pointer_with_a_nonzero_length() {
        let count = unsafe { dicta_native_utf8_scalar_count(std::ptr::null(), 4) };
        assert_eq!(count, 0);
    }

    #[test]
    fn ui_commands_report_when_host_is_not_running() {
        assert!(host::ui_snapshot().is_err());
        assert!(host::stop_recording().is_err());
        assert_eq!(dicta_native_host_model_install_quality(), -1);
        assert!(host::last_error().contains("not running"));
    }
}
