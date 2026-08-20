mod capture;
pub(crate) mod control;
pub(crate) mod environment;
mod media;

use crate::platform::NativeCallback;
use serde_json::json;
use std::ffi::CString;

pub(crate) use capture::{abort_recording, start_recording, stop_recording};
pub(crate) use media::{extract_audio, extract_poster};

fn emit(callback: NativeCallback, event: &str, message: &str) {
    let Ok(event) = CString::new(event) else {
        return;
    };
    let Ok(message) = CString::new(message) else {
        return;
    };
    callback(event.as_ptr(), message.as_ptr());
}

pub(crate) fn transcribe(
    input_path: &str,
    _language: &str,
    callback: NativeCallback,
) -> Result<(), String> {
    let payload = json!({
        "path": input_path,
        "error": "Linux uses Dicta's bundled local Whisper transcription"
    })
    .to_string();
    emit(callback, "transcription_error", &payload);
    Ok(())
}
