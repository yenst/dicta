use super::capture::NARRATION_FILTER;
use std::{path::Path, process::Command};

fn duration(path: &Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|duration| duration.is_finite() && *duration > 0.0)
}

pub(super) fn mux_spectacle(
    screen_path: &Path,
    audio_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    if !screen_path.is_file() {
        return Err("Spectacle did not produce a screen recording".to_string());
    }
    if !audio_path.is_file() {
        return Err("Microphone capture did not produce an audio track".to_string());
    }
    let audio_offset =
        (duration(audio_path).unwrap_or(0.0) - duration(screen_path).unwrap_or(0.0)).max(0.0);
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(screen_path)
        .args(["-ss", &format!("{audio_offset:.3}"), "-i"])
        .arg(audio_path)
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
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
            "-shortest",
            "-movflags",
            "+faststart",
        ])
        .arg(output_path)
        .output()
        .map_err(|error| format!("Could not combine screen and microphone capture: {error}"))?;
    if !output.status.success() || !output_path.is_file() {
        return Err(format!(
            "Could not finalize the MP4: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub(super) fn normalize_audio(output_path: &Path) -> Result<(), String> {
    let normalized_path = output_path.with_extension("normalizing.mp4");
    let _ = std::fs::remove_file(&normalized_path);
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(output_path)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-c:v",
            "copy",
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
        ])
        .arg(&normalized_path)
        .output()
        .map_err(|error| format!("Could not normalize microphone volume: {error}"))?;
    if !output.status.success() || !normalized_path.is_file() {
        let _ = std::fs::remove_file(&normalized_path);
        return Err(format!(
            "Could not normalize microphone volume: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    std::fs::rename(&normalized_path, output_path)
        .map_err(|error| format!("Could not save normalized microphone audio: {error}"))
}

pub(crate) fn extract_audio(input_path: &str, output_path: &str) -> bool {
    let _ = std::fs::remove_file(output_path);
    Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .arg("-i")
        .arg(input_path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(output_path)
        .status()
        .is_ok_and(|status| status.success() && Path::new(output_path).is_file())
}

pub(crate) fn extract_poster(input_path: &str, output_path: &str) -> bool {
    let _ = std::fs::remove_file(output_path);
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-ss",
            "0.1",
            "-i",
        ])
        .arg(input_path)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(output_path)
        .status()
        .is_ok_and(|status| status.success() && Path::new(output_path).is_file())
}
