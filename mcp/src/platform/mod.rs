#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
pub(crate) use macos::extract_frame;

#[cfg(target_os = "linux")]
pub(crate) use linux::extract_frame;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn extract_frame(
    _video_path: &std::path::Path,
    _seconds: f64,
    _output_path: &std::path::Path,
) -> Result<f64, String> {
    Err("Timestamped frame extraction is unavailable on this platform".to_string())
}
