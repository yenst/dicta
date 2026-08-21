use dicta_control::{VoiceNoteState, VoiceNoteStatus};
use serde::Serialize;
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct VoiceCaptureSnapshot {
    pub state: String,
    pub message: String,
    pub recording_id: Option<String>,
    pub note_id: Option<String>,
}

trait VoiceChild: Send {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> io::Result<Option<bool>>;
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<bool>;
}

impl VoiceChild for Child {
    fn id(&self) -> u32 {
        Child::id(self)
    }

    fn try_wait(&mut self) -> io::Result<Option<bool>> {
        Child::try_wait(self).map(|status| status.map(|status| status.success()))
    }

    fn kill(&mut self) -> io::Result<()> {
        Child::kill(self)
    }

    fn wait(&mut self) -> io::Result<bool> {
        Child::wait(self).map(|status| status.success())
    }
}

trait VoiceCapturePort: Send {
    fn start(&mut self, path: &Path) -> Result<Box<dyn VoiceChild>, String>;
    fn interrupt(&mut self, process_id: u32) -> Result<(), String>;
    fn sleep(&mut self, duration: Duration);
}

#[derive(Default)]
struct LinuxVoiceCapture;

impl VoiceCapturePort for LinuxVoiceCapture {
    fn start(&mut self, path: &Path) -> Result<Box<dyn VoiceChild>, String> {
        let program = std::env::var_os("DICTA_PW_RECORD_BIN")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| OsString::from("pw-record"));
        let child = Command::new(program)
            .args([
                "--media-category",
                "Capture",
                "--media-role",
                "Communication",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
            ])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                format!("Could not start microphone capture with pw-record: {error}")
            })?;
        Ok(Box::new(child))
    }

    fn interrupt(&mut self, process_id: u32) -> Result<(), String> {
        let output = Command::new("kill")
            .args(["-INT", &process_id.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("Could not stop microphone capture: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Could not stop microphone capture: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

trait VoiceNoteSubmitter: Send {
    fn submit(
        &mut self,
        recording_id: String,
        note_id: String,
        timestamp_seconds: f64,
        audio_path: String,
    ) -> Result<VoiceNoteStatus, String>;
    fn status(&mut self) -> Result<VoiceNoteStatus, String>;
    fn cancel(&mut self) -> Result<VoiceNoteStatus, String>;
}

#[derive(Default)]
struct RuntimeSubmitter;

impl VoiceNoteSubmitter for RuntimeSubmitter {
    fn submit(
        &mut self,
        recording_id: String,
        note_id: String,
        timestamp_seconds: f64,
        audio_path: String,
    ) -> Result<VoiceNoteStatus, String> {
        crate::host::transcribe_voice_note(recording_id, note_id, timestamp_seconds, audio_path)
    }

    fn status(&mut self) -> Result<VoiceNoteStatus, String> {
        crate::host::voice_note_status()
    }

    fn cancel(&mut self) -> Result<VoiceNoteStatus, String> {
        crate::host::cancel_voice_note()
    }
}

enum CaptureState {
    Idle,
    Recording {
        recording_id: String,
        note_id: String,
        timestamp_seconds: f64,
        path: PathBuf,
        child: Box<dyn VoiceChild>,
    },
    Processing {
        recording_id: String,
        note_id: String,
    },
    Terminal(VoiceCaptureSnapshot),
}

struct VoiceCaptureManager<P, S> {
    port: P,
    submitter: S,
    state: CaptureState,
    counter: u64,
    directory: Option<PathBuf>,
}

impl<P, S> VoiceCaptureManager<P, S>
where
    P: VoiceCapturePort,
    S: VoiceNoteSubmitter,
{
    fn new(port: P, submitter: S) -> Self {
        Self {
            port,
            submitter,
            state: CaptureState::Idle,
            counter: 0,
            directory: None,
        }
    }

    #[cfg(test)]
    fn with_directory(mut self, directory: PathBuf) -> Self {
        self.directory = Some(directory);
        self
    }

    fn start(&mut self, recording_id: String, timestamp_seconds: f64) -> Result<(), String> {
        if !matches!(self.state, CaptureState::Idle | CaptureState::Terminal(_)) {
            return Err("a voice note is already active".to_owned());
        }
        if recording_id.trim().is_empty()
            || !timestamp_seconds.is_finite()
            || timestamp_seconds < 0.0
        {
            return Err("voice-note recording or timestamp is invalid".to_owned());
        }
        let directory = self.directory.clone().map_or_else(
            || dicta_runtime::voice_note_directory().map_err(|error| error.to_string()),
            Ok,
        )?;
        fs::create_dir_all(&directory)
            .map_err(|error| format!("Could not prepare voice-note storage: {error}"))?;
        self.counter = self.counter.wrapping_add(1);
        let note_id = format!(
            "voice-{}-{}-{:016x}",
            std::process::id(),
            self.counter,
            timestamp_seconds.to_bits()
        );
        let path = directory.join(format!("{note_id}.wav"));
        if path.exists() {
            return Err("voice-note capture path already exists".to_owned());
        }
        let child = self.port.start(&path)?;
        self.state = CaptureState::Recording {
            recording_id,
            note_id,
            timestamp_seconds,
            path,
            child,
        };
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        let state = std::mem::replace(&mut self.state, CaptureState::Idle);
        let CaptureState::Recording {
            recording_id,
            note_id,
            timestamp_seconds,
            path,
            mut child,
        } = state
        else {
            self.state = state;
            return Err("no voice-note microphone capture is active".to_owned());
        };
        let stop_result = self.stop_child(&mut *child);
        if let Err(error) = stop_result {
            let _ = fs::remove_file(&path);
            self.state = CaptureState::Terminal(failed(error.clone(), &recording_id, &note_id));
            return Err(error);
        }
        set_private_file_permissions(&path)?;
        let audio_path = path.to_string_lossy().into_owned();
        match self.submitter.submit(
            recording_id.clone(),
            note_id.clone(),
            timestamp_seconds,
            audio_path,
        ) {
            Ok(status) if status.state == VoiceNoteState::Processing => {
                self.state = CaptureState::Processing {
                    recording_id,
                    note_id,
                };
                Ok(())
            }
            Ok(status) => {
                self.state = CaptureState::Terminal(from_runtime_status(status));
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_file(path);
                self.state = CaptureState::Terminal(failed(error.clone(), &recording_id, &note_id));
                Err(error)
            }
        }
    }

    fn stop_child(&mut self, child: &mut dyn VoiceChild) -> Result<(), String> {
        self.port.interrupt(child.id())?;
        for _ in 0..40 {
            match child.try_wait() {
                Ok(Some(true)) => return Ok(()),
                Ok(Some(false)) => {
                    return Err("Microphone capture exited before finalizing audio".to_owned())
                }
                Ok(None) => self.port.sleep(Duration::from_millis(50)),
                Err(error) => return Err(format!("Could not poll microphone capture: {error}")),
            }
        }
        child
            .kill()
            .map_err(|error| format!("Could not terminate microphone capture: {error}"))?;
        let _ = child.wait();
        Err("Microphone capture did not stop within two seconds".to_owned())
    }

    fn cancel(&mut self) -> Result<(), String> {
        let state = std::mem::replace(&mut self.state, CaptureState::Idle);
        match state {
            CaptureState::Recording {
                path, mut child, ..
            } => {
                let _ = self.stop_child(&mut *child);
                let _ = fs::remove_file(path);
                Ok(())
            }
            CaptureState::Processing { .. } => {
                let status = self.submitter.cancel()?;
                self.state = CaptureState::Terminal(from_runtime_status(status));
                Ok(())
            }
            CaptureState::Idle | CaptureState::Terminal(_) => Ok(()),
        }
    }

    fn snapshot(&mut self) -> VoiceCaptureSnapshot {
        if matches!(self.state, CaptureState::Processing { .. }) {
            match self.submitter.status() {
                Ok(status) if status.state != VoiceNoteState::Processing => {
                    self.state = CaptureState::Terminal(from_runtime_status(status));
                }
                Ok(_) => {}
                Err(error) => {
                    let (recording_id, note_id) = match &self.state {
                        CaptureState::Processing {
                            recording_id,
                            note_id,
                        } => (recording_id.clone(), note_id.clone()),
                        _ => unreachable!(),
                    };
                    self.state = CaptureState::Terminal(failed(error, &recording_id, &note_id));
                }
            }
        }
        match &self.state {
            CaptureState::Idle => VoiceCaptureSnapshot {
                state: "idle".to_owned(),
                ..VoiceCaptureSnapshot::default()
            },
            CaptureState::Recording {
                recording_id,
                note_id,
                ..
            } => VoiceCaptureSnapshot {
                state: "recording".to_owned(),
                message: "Listening…".to_owned(),
                recording_id: Some(recording_id.clone()),
                note_id: Some(note_id.clone()),
            },
            CaptureState::Processing {
                recording_id,
                note_id,
            } => VoiceCaptureSnapshot {
                state: "processing".to_owned(),
                message: "Transcribing voice note…".to_owned(),
                recording_id: Some(recording_id.clone()),
                note_id: Some(note_id.clone()),
            },
            CaptureState::Terminal(snapshot) => snapshot.clone(),
        }
    }
}

fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Could not secure voice-note audio: {error}"))?;
    }
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect voice-note audio: {error}"))?;
    if metadata.len() <= 44 {
        return Err("Microphone capture produced no audio".to_owned());
    }
    Ok(())
}

fn failed(message: String, recording_id: &str, note_id: &str) -> VoiceCaptureSnapshot {
    VoiceCaptureSnapshot {
        state: "failed".to_owned(),
        message,
        recording_id: Some(recording_id.to_owned()),
        note_id: Some(note_id.to_owned()),
    }
}

fn from_runtime_status(status: VoiceNoteStatus) -> VoiceCaptureSnapshot {
    VoiceCaptureSnapshot {
        state: match status.state {
            VoiceNoteState::Idle => "idle",
            VoiceNoteState::Processing => "processing",
            VoiceNoteState::Complete => "complete",
            VoiceNoteState::Failed => "failed",
            VoiceNoteState::Cancelling => "cancelling",
        }
        .to_owned(),
        message: status.message,
        recording_id: status.recording_id,
        note_id: status.note_id,
    }
}

type SystemManager = VoiceCaptureManager<LinuxVoiceCapture, RuntimeSubmitter>;

fn manager() -> &'static Mutex<SystemManager> {
    static MANAGER: OnceLock<Mutex<SystemManager>> = OnceLock::new();
    MANAGER.get_or_init(|| {
        Mutex::new(VoiceCaptureManager::new(
            LinuxVoiceCapture,
            RuntimeSubmitter,
        ))
    })
}

fn lock_manager() -> MutexGuard<'static, SystemManager> {
    manager()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn start(recording_id: String, timestamp_seconds: f64) -> Result<(), String> {
    lock_manager().start(recording_id, timestamp_seconds)
}

pub fn stop() -> Result<(), String> {
    lock_manager().stop()
}

pub fn cancel() -> Result<(), String> {
    lock_manager().cancel()
}

pub fn snapshot() -> VoiceCaptureSnapshot {
    lock_manager().snapshot()
}

pub fn shutdown() {
    let _ = cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakePort {
        paths: Arc<Mutex<Vec<PathBuf>>>,
    }

    struct FakeChild;

    impl VoiceChild for FakeChild {
        fn id(&self) -> u32 {
            42
        }
        fn try_wait(&mut self) -> io::Result<Option<bool>> {
            Ok(Some(true))
        }
        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn wait(&mut self) -> io::Result<bool> {
            Ok(true)
        }
    }

    impl VoiceCapturePort for FakePort {
        fn start(&mut self, path: &Path) -> Result<Box<dyn VoiceChild>, String> {
            fs::write(path, vec![0_u8; 128]).map_err(|error| error.to_string())?;
            self.paths.lock().unwrap().push(path.to_path_buf());
            Ok(Box::new(FakeChild))
        }
        fn interrupt(&mut self, _process_id: u32) -> Result<(), String> {
            Ok(())
        }
        fn sleep(&mut self, _duration: Duration) {}
    }

    #[derive(Default)]
    struct FakeSubmitter {
        status: VoiceNoteStatus,
        audio_path: Option<PathBuf>,
    }

    impl VoiceNoteSubmitter for FakeSubmitter {
        fn submit(
            &mut self,
            recording_id: String,
            note_id: String,
            _timestamp_seconds: f64,
            audio_path: String,
        ) -> Result<VoiceNoteStatus, String> {
            self.audio_path = Some(PathBuf::from(audio_path));
            self.status = VoiceNoteStatus {
                state: VoiceNoteState::Processing,
                recording_id: Some(recording_id),
                note_id: Some(note_id),
                message: "processing".to_owned(),
            };
            Ok(self.status.clone())
        }
        fn status(&mut self) -> Result<VoiceNoteStatus, String> {
            Ok(self.status.clone())
        }
        fn cancel(&mut self) -> Result<VoiceNoteStatus, String> {
            if let Some(path) = self.audio_path.take() {
                let _ = fs::remove_file(path);
            }
            self.status.state = VoiceNoteState::Cancelling;
            self.status.message = "cancelling".to_owned();
            Ok(self.status.clone())
        }
    }

    #[test]
    fn microphone_capture_is_bounded_and_cancel_cleans_the_temp_file() {
        let port = FakePort::default();
        let paths = Arc::clone(&port.paths);
        let directory = std::env::temp_dir().join(format!(
            "dicta-voice-capture-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut manager = VoiceCaptureManager::new(port, FakeSubmitter::default())
            .with_directory(directory.clone());
        manager.start("recording".to_owned(), 12.0).unwrap();
        assert_eq!(manager.snapshot().state, "recording");
        manager.stop().unwrap();
        assert_eq!(manager.snapshot().state, "processing");
        let path = paths.lock().unwrap()[0].clone();
        assert!(path.is_file());
        manager.cancel().unwrap();
        assert!(!path.exists());
        assert_eq!(manager.snapshot().state, "cancelling");
        fs::remove_dir_all(directory).unwrap();
    }
}
