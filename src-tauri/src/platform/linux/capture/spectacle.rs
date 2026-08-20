use super::{silent_command, stop_stdin, RecorderProcess};
use std::{
    path::Path,
    process::{Command, Stdio},
};

pub(super) fn start(output_path: &Path) -> Result<RecorderProcess, String> {
    let screen_path = output_path.with_extension("screen.webm");
    let audio_path = output_path.with_extension("audio.wav");
    let _ = std::fs::remove_file(&screen_path);
    let _ = std::fs::remove_file(&audio_path);

    let service = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.kde.Spectacle",
            "--object-path",
            "/",
            "--method",
            "org.freedesktop.DBus.Peer.Ping",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Could not connect to Spectacle over DBus: {error}"))?;
    if !service.status.success() {
        return Err(format!(
            "Could not activate Spectacle: {}",
            String::from_utf8_lossy(&service.stderr).trim()
        ));
    }

    let mut audio_command = silent_command("ffmpeg");
    audio_command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "pulse",
        "-i",
        "default",
        "-ac",
        "1",
        "-ar",
        "48000",
        "-c:a",
        "pcm_s16le",
    ]);
    audio_command.arg(&audio_path);
    let audio = audio_command
        .spawn()
        .map_err(|error| format!("Could not start microphone capture: {error}"))?;

    let activation = Command::new("spectacle")
        .args(["--record", "s", "--background", "--nonotify", "--output"])
        .arg(&screen_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    match activation {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let mut audio = audio;
            stop_stdin(&mut audio);
            let _ = audio.wait();
            return Err(format!(
                "Could not start Spectacle screen capture: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Err(error) => {
            let mut audio = audio;
            stop_stdin(&mut audio);
            let _ = audio.wait();
            return Err(format!("Could not start Spectacle screen capture: {error}"));
        }
    }

    Ok(RecorderProcess::Spectacle {
        audio,
        screen_path,
        audio_path,
        output_path: output_path.to_path_buf(),
    })
}

pub(super) fn stop() -> Result<(), String> {
    let output = Command::new("spectacle")
        .args(["--background", "--nonotify"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Could not ask Spectacle to finish recording: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Spectacle could not finish recording: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}
