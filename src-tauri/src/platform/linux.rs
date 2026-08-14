use crate::platform::NativeCallback;
use serde_json::json;
use std::{
    env,
    ffi::CString,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

static RECORDER: OnceLock<Mutex<Option<RecorderProcess>>> = OnceLock::new();
const NARRATION_FILTER: &str = "loudnorm=I=-16:LRA=11:TP=-1.5";

enum RecorderProcess {
    Direct {
        child: Child,
        output_path: PathBuf,
        stop_with_interrupt: bool,
        normalize_after: bool,
    },
    Spectacle {
        audio: Child,
        screen_path: PathBuf,
        audio_path: PathBuf,
        output_path: PathBuf,
    },
}

fn recorder() -> &'static Mutex<Option<RecorderProcess>> {
    RECORDER.get_or_init(|| Mutex::new(None))
}

fn executable_exists(name: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|directory| directory.join(name).is_file()))
}

fn is_wayland_session() -> bool {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if session_type.eq_ignore_ascii_case("x11") {
        return false;
    }
    if env::var("GDK_BACKEND")
        .map(|backend| backend.eq_ignore_ascii_case("x11"))
        .unwrap_or(false)
    {
        return false;
    }
    session_type.eq_ignore_ascii_case("wayland") || env::var_os("WAYLAND_DISPLAY").is_some()
}

fn emit(callback: NativeCallback, event: &str, message: &str) {
    let Ok(event) = CString::new(event) else {
        return;
    };
    let Ok(message) = CString::new(message) else {
        return;
    };
    callback(event.as_ptr(), message.as_ptr());
}

fn silent_command(name: &str) -> Command {
    let mut command = Command::new(name);
    command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());
    command
}

fn x11_size() -> Option<String> {
    let output = Command::new("xrandr").arg("--current").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let marker = " current ";
    let current = text.lines().find_map(|line| {
        let remainder = line.split_once(marker)?.1;
        let value = remainder.split(',').next()?.trim();
        let mut parts = value.split_whitespace();
        let width = parts.next()?.parse::<u32>().ok()?;
        if parts.next()? != "x" {
            return None;
        }
        let height = parts.next()?.parse::<u32>().ok()?;
        Some(format!("{width}x{height}"))
    });
    current
}

fn start_ffmpeg_x11(output_path: &Path) -> Result<RecorderProcess, String> {
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

fn start_wf_recorder(output_path: &Path) -> Result<RecorderProcess, String> {
    let mut command = silent_command("wf-recorder");
    command.args(["--audio", "--file"]);
    command.arg(output_path);
    let child = command
        .spawn()
        .map_err(|error| format!("Could not start wf-recorder: {error}"))?;
    Ok(RecorderProcess::Direct {
        child,
        output_path: output_path.to_path_buf(),
        stop_with_interrupt: true,
        normalize_after: true,
    })
}

fn start_spectacle(output_path: &Path) -> Result<RecorderProcess, String> {
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
    let audio = match audio_command.spawn() {
        Ok(audio) => audio,
        Err(error) => return Err(format!("Could not start microphone capture: {error}")),
    };

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

fn process_exited(process: &mut RecorderProcess) -> Result<bool, String> {
    let status = match process {
        RecorderProcess::Direct { child, .. } => child.try_wait(),
        RecorderProcess::Spectacle { audio, .. } => audio.try_wait(),
    }
    .map_err(|error| format!("Could not inspect the screen recorder: {error}"))?;
    Ok(status.is_some())
}

fn abort_process(process: &mut RecorderProcess) {
    match process {
        RecorderProcess::Direct { child, .. } => {
            interrupt(child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
        RecorderProcess::Spectacle { audio, .. } => {
            let _ = stop_spectacle_capture();
            stop_stdin(audio);
            let _ = audio.kill();
            let _ = audio.wait();
        }
    }
}

pub(crate) fn start_recording(output_path: &str, callback: NativeCallback) -> Result<(), String> {
    if !executable_exists("ffmpeg") {
        return Err(
            "Linux recording requires FFmpeg. Install the `ffmpeg` package and try again."
                .to_string(),
        );
    }
    let mut slot = recorder()
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if slot.is_some() {
        return Err("A Linux screen recorder is already running".to_string());
    }

    let output_path = PathBuf::from(output_path);
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let override_backend = env::var("DICTA_SCREEN_RECORDER").unwrap_or_default();
    let mut process = if override_backend == "ffmpeg-x11" {
        start_ffmpeg_x11(&output_path)?
    } else if override_backend == "spectacle" {
        start_spectacle(&output_path)?
    } else if override_backend == "wf-recorder" {
        start_wf_recorder(&output_path)?
    } else if !is_wayland_session() {
        start_ffmpeg_x11(&output_path)?
    } else if desktop.to_ascii_lowercase().contains("kde") && executable_exists("spectacle") {
        start_spectacle(&output_path)?
    } else if executable_exists("wf-recorder") {
        start_wf_recorder(&output_path)?
    } else {
        return Err("This Wayland desktop needs a supported recorder. Install `wf-recorder`, or set DICTA_SCREEN_RECORDER=ffmpeg-x11 when an X11 display is available.".to_string());
    };

    thread::sleep(Duration::from_millis(800));
    if process_exited(&mut process)? {
        abort_process(&mut process);
        return Err("The Linux screen recorder exited before capture started. Check screen-recording and microphone permissions.".to_string());
    }
    let needs_screen_selection = matches!(&process, RecorderProcess::Spectacle { .. });
    *slot = Some(process);
    drop(slot);
    emit(
        callback,
        "started",
        if needs_screen_selection {
            "Click a window on the screen you want Plasma to record"
        } else {
            "Screen and microphone capture started"
        },
    );
    Ok(())
}

fn interrupt(pid: u32) {
    let _ = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn stop_stdin(child: &mut Child) {
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(b"q\n");
        let _ = stdin.flush();
    }
}

fn wait_for_exit(child: &mut Child) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("Could not finalize capture: {error}"))?
            .is_some()
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("The screen recorder did not stop cleanly".to_string());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn stop_spectacle_capture() -> Result<(), String> {
    let output = Command::new("spectacle")
        .args(["--background", "--nonotify"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Could not ask Spectacle to finish recording: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Spectacle could not finish recording: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn wait_for_stable_file(path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last_size = 0;
    let mut stable_checks = 0;
    loop {
        let size = std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if size > 0 && size == last_size {
            stable_checks += 1;
            if stable_checks >= 4 {
                return Ok(());
            }
        } else {
            stable_checks = 0;
            last_size = size;
        }
        if Instant::now() >= deadline {
            return Err("Spectacle did not finish the screen recording. Select a screen when Plasma prompts, then try again.".to_string());
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn media_duration(path: &Path) -> Option<f64> {
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

fn mux_spectacle(screen_path: &Path, audio_path: &Path, output_path: &Path) -> Result<(), String> {
    if !screen_path.is_file() {
        return Err("Spectacle did not produce a screen recording".to_string());
    }
    if !audio_path.is_file() {
        return Err("Microphone capture did not produce an audio track".to_string());
    }
    let screen_duration = media_duration(screen_path).unwrap_or(0.0);
    let audio_duration = media_duration(audio_path).unwrap_or(0.0);
    let audio_offset = (audio_duration - screen_duration).max(0.0);
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
    if output.status.success() && output_path.is_file() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr);
        Err(format!("Could not finalize the MP4: {}", message.trim()))
    }
}

fn normalize_recording_audio(output_path: &Path) -> Result<(), String> {
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

pub(crate) fn stop_recording(callback: NativeCallback) -> Result<(), String> {
    let process = recorder()
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?
        .take()
        .ok_or_else(|| "No Linux screen recorder is running".to_string())?;

    let result = match process {
        RecorderProcess::Direct {
            mut child,
            output_path,
            stop_with_interrupt,
            normalize_after,
        } => (|| -> Result<(), String> {
            if stop_with_interrupt {
                interrupt(child.id());
            } else {
                stop_stdin(&mut child);
            }
            wait_for_exit(&mut child)?;
            if !output_path.is_file() {
                return Err("The Linux recorder did not produce an MP4 file".to_string());
            }
            if normalize_after {
                normalize_recording_audio(&output_path)?;
            }
            Ok(())
        })(),
        RecorderProcess::Spectacle {
            mut audio,
            screen_path,
            audio_path,
            output_path,
        } => {
            let spectacle_result = stop_spectacle_capture();
            stop_stdin(&mut audio);
            let audio_result = wait_for_exit(&mut audio);
            let result = spectacle_result
                .and(audio_result)
                .and_then(|_| wait_for_stable_file(&screen_path))
                .and_then(|_| mux_spectacle(&screen_path, &audio_path, &output_path));
            if result.is_ok() {
                let _ = std::fs::remove_file(&screen_path);
                let _ = std::fs::remove_file(&audio_path);
                result
            } else {
                result.map_err(|error| {
                    format!(
                        "{error}. Temporary captures were kept at {} and {}",
                        screen_path.display(),
                        audio_path.display()
                    )
                })
            }
        }
    };

    match result {
        Ok(()) => emit(callback, "finished", "Recording saved"),
        Err(error) => emit(callback, "error", &error),
    }
    Ok(())
}

pub(crate) fn transcribe(
    input_path: &str,
    _language: &str,
    callback: NativeCallback,
) -> Result<(), String> {
    let payload = json!({
        "path": input_path,
        "error": "Linux uses Dicta's bundled local Whisper transcription"
    })
    .to_string();
    emit(callback, "transcription_error", &payload);
    Ok(())
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
