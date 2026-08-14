#[cfg(not(target_os = "macos"))]
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(crate) use macos::extract_frame;

#[cfg(not(target_os = "macos"))]
pub(crate) fn extract_frame(
    _video_path: &Path,
    _seconds: f64,
    _output_path: &Path,
) -> Result<f64, String> {
    Err("Timestamped frame extraction is unavailable on this platform".to_string())
}
