use std::{path::Path, process::Command};

pub(crate) fn extract_frame(
    video_path: &Path,
    seconds: f64,
    output_path: &Path,
) -> Result<f64, String> {
    let _ = std::fs::remove_file(output_path);
    let output = extraction_command(video_path, seconds, output_path)
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

fn extraction_command(video_path: &Path, seconds: f64, output_path: &Path) -> Command {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss"])
        .arg(format!("{:.3}", seconds.max(0.0)))
        .arg("-i")
        .arg(video_path)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(output_path);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_receives_paths_as_opaque_arguments_without_a_shell() {
        let input = Path::new("/tmp/input;touch injected.mp4");
        let output = Path::new("/tmp/output $(touch injected).jpg");
        let command = extraction_command(input, 1.25, output);
        assert_eq!(command.get_program(), "ffmpeg");
        let arguments = command.get_args().collect::<Vec<_>>();
        assert!(arguments.contains(&input.as_os_str()));
        assert!(arguments.contains(&output.as_os_str()));
        assert_eq!(
            arguments
                .windows(2)
                .find(|pair| pair[0] == "-ss")
                .map(|pair| pair[1]),
            Some(std::ffi::OsStr::new("1.250"))
        );
    }
}
