#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(crate) use linux::extract_frame;

#[cfg(not(target_os = "linux"))]
pub(crate) fn extract_frame(
    _video_path: &std::path::Path,
    _seconds: f64,
    _output_path: &std::path::Path,
) -> Result<f64, String> {
    Err("Timestamped frame extraction is unavailable on this platform".to_string())
}
