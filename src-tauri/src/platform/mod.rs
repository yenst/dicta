use std::os::raw::c_char;

pub(crate) type NativeCallback = extern "C" fn(*const c_char, *const c_char);

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::{
    extract_audio, extract_poster, start_recording, stop_recording, transcribe,
};

#[cfg(not(target_os = "macos"))]
pub(crate) fn start_recording(_output_path: &str, _callback: NativeCallback) -> Result<(), String> {
    Err("Dicta recording currently supports macOS only".to_string())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn stop_recording(_callback: NativeCallback) -> Result<(), String> {
    Err("Dicta recording currently supports macOS only".to_string())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn transcribe(
    _input_path: &str,
    _language: &str,
    _callback: NativeCallback,
) -> Result<(), String> {
    Err("Native transcription is unavailable on this platform".to_string())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn extract_audio(_input_path: &str, _output_path: &str) -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn extract_poster(_input_path: &str, _output_path: &str) -> bool {
    false
}
