mod ffmpeg_x11;
mod spectacle;
mod wf_recorder;

use super::{emit, environment::executable_exists, environment::CaptureEnvironment, media};
use crate::platform::NativeCallback;
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

static RECORDER: OnceLock<Mutex<Option<RecorderProcess>>> = OnceLock::new();
pub(super) const NARRATION_FILTER: &str = "loudnorm=I=-16:LRA=11:TP=-1.5";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecorderBackend {
    FfmpegX11,
    Spectacle,
    WfRecorder,
}

impl RecorderBackend {
    fn detect(override_backend: &str, environment: CaptureEnvironment) -> Result<Self, String> {
        match override_backend {
            "ffmpeg-x11" => return Ok(Self::FfmpegX11),
            "spectacle" => return Ok(Self::Spectacle),
            "wf-recorder" => return Ok(Self::WfRecorder),
            "" => {}
            value => {
                return Err(format!(
                    "Unsupported DICTA_SCREEN_RECORDER value `{value}`. Use `ffmpeg-x11`, `spectacle`, or `wf-recorder`."
                ));
            }
        }
        if !environment.is_wayland {
            return Ok(Self::FfmpegX11);
        }
        if environment.is_kde && environment.spectacle_available {
            return Ok(Self::Spectacle);
        }
        if environment.wf_recorder_available {
            return Ok(Self::WfRecorder);
        }
        Err("This Wayland desktop needs a supported recorder. Install `wf-recorder`, or set DICTA_SCREEN_RECORDER=ffmpeg-x11 when an X11 display is available.".to_string())
    }

    fn start(self, output_path: &Path) -> Result<RecorderProcess, String> {
        match self {
            Self::FfmpegX11 => ffmpeg_x11::start(output_path),
            Self::Spectacle => spectacle::start(output_path),
            Self::WfRecorder => wf_recorder::start(output_path),
        }
    }

    fn started_message(self) -> &'static str {
        match self {
            Self::Spectacle => "Click a window on the screen you want Plasma to record",
            Self::FfmpegX11 | Self::WfRecorder => "Display and microphone capture started",
        }
    }
}

pub(super) enum RecorderProcess {
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

impl Drop for RecorderProcess {
    fn drop(&mut self) {
        abort_process(self);
    }
}

fn recorder() -> &'static Mutex<Option<RecorderProcess>> {
    RECORDER.get_or_init(|| Mutex::new(None))
}

pub(super) fn silent_command(name: &str) -> Command {
    let mut command = Command::new(name);
    command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());
    command
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
            if child.try_wait().ok().flatten().is_none() {
                interrupt(child.id());
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        RecorderProcess::Spectacle { audio, .. } => {
            if audio.try_wait().ok().flatten().is_none() {
                let _ = spectacle::stop();
                stop_stdin(audio);
                let _ = audio.kill();
                let _ = audio.wait();
            }
        }
    }
}

pub(crate) fn abort_recording() {
    let process = recorder()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    drop(process);
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
    let backend = RecorderBackend::detect(
        &env::var("DICTA_SCREEN_RECORDER").unwrap_or_default(),
        CaptureEnvironment::current(),
    )?;
    let mut process = backend.start(&output_path)?;
    thread::sleep(Duration::from_millis(800));
    if process_exited(&mut process)? {
        abort_process(&mut process);
        return Err("The Linux screen recorder exited before capture started. Check screen-recording and microphone permissions.".to_string());
    }
    *slot = Some(process);
    drop(slot);
    emit(callback, "started", backend.started_message());
    Ok(())
}

fn interrupt(pid: u32) {
    let _ = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub(super) fn stop_stdin(child: &mut Child) {
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

pub(crate) fn stop_recording(callback: NativeCallback) -> Result<(), String> {
    let mut process = recorder()
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?
        .take()
        .ok_or_else(|| "No Linux screen recorder is running".to_string())?;
    let result = match &mut process {
        RecorderProcess::Direct {
            child,
            output_path,
            stop_with_interrupt,
            normalize_after,
        } => (|| -> Result<(), String> {
            if *stop_with_interrupt {
                interrupt(child.id());
            } else {
                stop_stdin(child);
            }
            wait_for_exit(child)?;
            if !output_path.is_file() {
                return Err("The Linux recorder did not produce an MP4 file".to_string());
            }
            if *normalize_after {
                media::normalize_audio(output_path)?;
            }
            Ok(())
        })(),
        RecorderProcess::Spectacle {
            audio,
            screen_path,
            audio_path,
            output_path,
        } => {
            let result = spectacle::stop()
                .and_then(|_| {
                    stop_stdin(audio);
                    wait_for_exit(audio)
                })
                .and_then(|_| wait_for_stable_file(screen_path))
                .and_then(|_| media::mux_spectacle(screen_path, audio_path, output_path));
            if result.is_ok() {
                let _ = std::fs::remove_file(screen_path);
                let _ = std::fs::remove_file(audio_path);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wayland_environment() -> CaptureEnvironment {
        CaptureEnvironment {
            is_wayland: true,
            is_kde: false,
            spectacle_available: false,
            wf_recorder_available: true,
        }
    }

    #[test]
    fn recorder_overrides_bypass_desktop_detection() {
        let environment = CaptureEnvironment {
            is_wayland: false,
            is_kde: false,
            spectacle_available: false,
            wf_recorder_available: false,
        };
        assert_eq!(
            RecorderBackend::detect("ffmpeg-x11", environment).unwrap(),
            RecorderBackend::FfmpegX11
        );
        assert_eq!(
            RecorderBackend::detect("spectacle", environment).unwrap(),
            RecorderBackend::Spectacle
        );
        assert_eq!(
            RecorderBackend::detect("wf-recorder", environment).unwrap(),
            RecorderBackend::WfRecorder
        );
    }

    #[test]
    fn recorder_detection_stays_specific_to_the_linux_session() {
        let x11 = CaptureEnvironment {
            is_wayland: false,
            ..wayland_environment()
        };
        assert_eq!(
            RecorderBackend::detect("", x11).unwrap(),
            RecorderBackend::FfmpegX11
        );
        let kde = CaptureEnvironment {
            is_kde: true,
            spectacle_available: true,
            ..wayland_environment()
        };
        assert_eq!(
            RecorderBackend::detect("", kde).unwrap(),
            RecorderBackend::Spectacle
        );
        assert_eq!(
            RecorderBackend::detect("", wayland_environment()).unwrap(),
            RecorderBackend::WfRecorder
        );
    }

    #[test]
    fn unsupported_recorder_configuration_is_reported() {
        assert!(RecorderBackend::detect("unknown", wayland_environment())
            .unwrap_err()
            .contains("Unsupported DICTA_SCREEN_RECORDER"));
        let unavailable = CaptureEnvironment {
            wf_recorder_available: false,
            ..wayland_environment()
        };
        assert!(RecorderBackend::detect("", unavailable)
            .unwrap_err()
            .contains("needs a supported recorder"));
    }

    #[test]
    fn dropping_a_recorder_process_aborts_its_child() {
        let child = Command::new("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        let process = RecorderProcess::Direct {
            child,
            output_path: PathBuf::from("/tmp/dicta-drop-test.mp4"),
            stop_with_interrupt: true,
            normalize_after: false,
        };
        drop(process);
        let still_running = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        assert!(!still_running, "recorder child {pid} survived Drop");
    }
}
