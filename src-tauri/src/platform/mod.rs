use std::os::raw::c_char;

pub(crate) mod shell;

pub(crate) type NativeCallback = extern "C" fn(*const c_char, *const c_char);

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(target_os = "macos")]
pub(crate) use macos::{
    extract_audio, extract_poster, start_recording, stop_recording, transcribe,
};

#[cfg(target_os = "linux")]
pub(crate) use linux::{
    abort_recording, extract_audio, extract_poster, start_recording, stop_recording, transcribe,
};

#[cfg(not(target_os = "linux"))]
pub(crate) fn abort_recording() {}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn start_recording(_output_path: &str, _callback: NativeCallback) -> Result<(), String> {
    Err("Dicta recording currently supports macOS and Linux only".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn stop_recording(_callback: NativeCallback) -> Result<(), String> {
    Err("Dicta recording currently supports macOS and Linux only".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn transcribe(
    _input_path: &str,
    _language: &str,
    _callback: NativeCallback,
) -> Result<(), String> {
    Err("Native transcription is unavailable on this platform".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn extract_audio(_input_path: &str, _output_path: &str) -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn extract_poster(_input_path: &str, _output_path: &str) -> bool {
    false
}
