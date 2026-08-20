use super::{silent_command, RecorderProcess, NARRATION_FILTER};
use std::{env, path::Path, process::Command};

fn x11_size() -> Option<String> {
    let output = Command::new("xrandr").arg("--current").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().find_map(|line| {
        let remainder = line.split_once(" current ")?.1;
        let value = remainder.split(',').next()?.trim();
        let mut parts = value.split_whitespace();
        let width = parts.next()?.parse::<u32>().ok()?;
        if parts.next()? != "x" {
            return None;
        }
        let height = parts.next()?.parse::<u32>().ok()?;
        Some(format!("{width}x{height}"))
    })
}

pub(super) fn start(output_path: &Path) -> Result<RecorderProcess, String> {
    let display = env::var("DISPLAY")
        .map_err(|_| "X11 capture needs the DISPLAY environment variable".to_string())?;
    let size = x11_size().unwrap_or_else(|| "1920x1080".to_string());
    let mut command = silent_command("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "x11grab",
        "-framerate",
        "30",
        "-video_size",
        &size,
        "-i",
        &display,
        "-f",
        "pulse",
        "-i",
        "default",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-crf",
        "23",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "128k",
        "-af",
        NARRATION_FILTER,
        "-ar",
        "48000",
        "-movflags",
        "+faststart",
    ]);
    command.arg(output_path);
    let child = command
        .spawn()
        .map_err(|error| format!("Could not start FFmpeg screen capture: {error}"))?;
    Ok(RecorderProcess::Direct {
        child,
        output_path: output_path.to_path_buf(),
        stop_with_interrupt: false,
        normalize_after: false,
    })
}
