use std::{path::Path, process::Command};

pub(crate) fn extract_frame(
    video_path: &Path,
    seconds: f64,
    output_path: &Path,
) -> Result<f64, String> {
    let _ = std::fs::remove_file(output_path);
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss"])
        .arg(format!("{:.3}", seconds.max(0.0)))
        .arg("-i")
        .arg(video_path)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(output_path)
        .output()
        .map_err(|error| {
            format!(
                "Could not start FFmpeg to extract a frame from `{}`: {error}",
                video_path.display()
            )
        })?;
    if output.status.success() && output_path.is_file() {
        Ok(seconds.max(0.0))
    } else {
        let message = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "Could not extract a screenshot at {} from `{}`: {}",
            dicta_core::transcript::format_timestamp(seconds),
            video_path.display(),
            message.trim()
        ))
    }
}
