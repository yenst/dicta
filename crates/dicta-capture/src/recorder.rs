use crate::{
    command::{CaptureChild, CommandPlan, Platform},
    discovery::CaptureCapabilities,
    error::CaptureError,
    plan::{capture_plan, CaptureBackend, CaptureConfig},
};
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_RECORDING_DURATION: Duration = Duration::from_mins(20);
const STOP_TIMEOUT: Duration = Duration::from_secs(8);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    User,
    Deadline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureArtifact {
    pub path: PathBuf,
    pub duration: Duration,
    pub backend: CaptureBackend,
    pub output_name: String,
    pub geometry: crate::discovery::Geometry,
    pub scale_milli: u32,
    pub encoded_pixel_size: (u32, u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PollOutcome {
    Idle,
    Running {
        elapsed: Duration,
        remaining: Duration,
    },
    Stopped {
        reason: StopReason,
        artifact: CaptureArtifact,
    },
}

struct ActiveCapture {
    child: Box<dyn CaptureChild>,
    started_at: Instant,
    destination: PathBuf,
    staging_destination: PathBuf,
    output_name: String,
    geometry: crate::discovery::Geometry,
    scale_milli: u32,
    encoded_pixel_size: (u32, u32),
    backend: CaptureBackend,
    program: String,
}

pub struct Recorder<P: Platform> {
    platform: P,
    active: Option<ActiveCapture>,
}

impl<P: Platform> Recorder<P> {
    #[must_use]
    pub const fn new(platform: P) -> Self {
        Self {
            platform,
            active: None,
        }
    }

    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    #[must_use]
    pub fn active_backend(&self) -> Option<CaptureBackend> {
        self.active.as_ref().map(|active| active.backend)
    }

    #[must_use]
    pub const fn platform(&self) -> &P {
        &self.platform
    }

    /// Starts one validated recorder process using deterministic backend selection.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, spawn, or early-process-exit error.
    pub fn start(
        &mut self,
        capabilities: &CaptureCapabilities,
        config: &CaptureConfig,
    ) -> Result<(), CaptureError> {
        self.start_at(capabilities, config, Instant::now())
    }

    /// Starts capture using an explicit monotonic start instant.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, spawn, or early-process-exit error.
    pub fn start_at(
        &mut self,
        capabilities: &CaptureCapabilities,
        config: &CaptureConfig,
        started_at: Instant,
    ) -> Result<(), CaptureError> {
        if self.active.is_some() {
            return Err(CaptureError::AlreadyRecording);
        }
        let plan = capture_plan(capabilities, config)?;
        let backend = plan.backend();
        if config.destination.exists() {
            return Err(CaptureError::DestinationExists(
                config.destination.display().to_string(),
            ));
        }
        if config.staging_destination.exists() {
            return Err(CaptureError::StagingExists(
                config.staging_destination.display().to_string(),
            ));
        }
        let program = plan.command().program().to_string_lossy().into_owned();
        let mut child = self
            .platform
            .spawn(plan.command())
            .map_err(|source| CaptureError::command_io(program.clone(), source))?;
        match child.try_wait() {
            Ok(Some(exit)) => {
                let _ = cleanup_staging(&config.staging_destination);
                return Err(CaptureError::RecorderExited { code: exit.code });
            }
            Ok(None) => {}
            Err(source) => {
                let error = CaptureError::command_io(program.clone(), source);
                let _ = terminate_child(&mut *child, &program);
                let _ = cleanup_staging(&config.staging_destination);
                return Err(error);
            }
        }
        self.active = Some(ActiveCapture {
            child,
            started_at,
            destination: config.destination.clone(),
            staging_destination: config.staging_destination.clone(),
            output_name: plan.output_name().to_string(),
            geometry: plan.geometry(),
            scale_milli: plan.scale_milli(),
            encoded_pixel_size: plan.encoded_pixel_size(),
            backend,
            program,
        });
        Ok(())
    }

    /// Reconciles process state and the recording deadline.
    ///
    /// # Errors
    ///
    /// Returns a process or finalization error. A deadline poll stops capture.
    pub fn poll(&mut self) -> Result<PollOutcome, CaptureError> {
        self.poll_at(Instant::now())
    }

    /// Reconciles state at an explicit monotonic instant.
    ///
    /// # Errors
    ///
    /// Returns a process or finalization error. A deadline poll stops capture.
    pub fn poll_at(&mut self, now: Instant) -> Result<PollOutcome, CaptureError> {
        let Some(active) = self.active.as_mut() else {
            return Ok(PollOutcome::Idle);
        };
        if let Some(exit) = active
            .child
            .try_wait()
            .map_err(|source| CaptureError::command_io(active.program.clone(), source))?
        {
            if let Some(active) = self.active.take() {
                let _ = cleanup_staging(&active.staging_destination);
            }
            return Err(CaptureError::RecorderExited { code: exit.code });
        }
        let elapsed = now.saturating_duration_since(active.started_at);
        if elapsed >= MAX_RECORDING_DURATION {
            let artifact = self.stop_active(now, StopReason::Deadline)?;
            return Ok(PollOutcome::Stopped {
                reason: StopReason::Deadline,
                artifact,
            });
        }
        Ok(PollOutcome::Running {
            elapsed,
            remaining: MAX_RECORDING_DURATION.saturating_sub(elapsed),
        })
    }

    /// Gracefully stops and verifies the recording artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when no capture runs, signaling or waiting fails, or
    /// the expected output file is missing.
    pub fn stop(&mut self) -> Result<CaptureArtifact, CaptureError> {
        self.stop_active(Instant::now(), StopReason::User)
    }

    /// Forcefully stops and reaps the active child, if any.
    ///
    /// # Errors
    ///
    /// Returns an operating-system process-control error.
    pub fn abort(&mut self) -> Result<(), CaptureError> {
        let Some(mut active) = self.active.take() else {
            return Ok(());
        };
        let process_result = terminate_child(&mut *active.child, &active.program);
        let cleanup_result = cleanup_staging(&active.staging_destination);
        process_result.and(cleanup_result)
    }

    fn stop_active(
        &mut self,
        stopped_at: Instant,
        _reason: StopReason,
    ) -> Result<CaptureArtifact, CaptureError> {
        let mut active = self.active.take().ok_or(CaptureError::NotRecording)?;
        if let Err(error) = send_interrupt(&mut self.platform, active.child.id()) {
            let _ = terminate_child(&mut *active.child, &active.program);
            let _ = cleanup_staging(&active.staging_destination);
            return Err(error);
        }
        let wait_result = wait_for_exit(&mut self.platform, &mut *active.child, &active.program);
        if let Err(error) = wait_result {
            let _ = terminate_child(&mut *active.child, &active.program);
            let _ = cleanup_staging(&active.staging_destination);
            return Err(error);
        }
        if let Err(error) = promote_staging(&active.staging_destination, &active.destination) {
            let _ = cleanup_staging(&active.staging_destination);
            return Err(error);
        }
        Ok(CaptureArtifact {
            path: active.destination,
            duration: stopped_at.saturating_duration_since(active.started_at),
            backend: active.backend,
            output_name: active.output_name,
            geometry: active.geometry,
            scale_milli: active.scale_milli,
            encoded_pixel_size: active.encoded_pixel_size,
        })
    }
}

impl<P: Platform> Drop for Recorder<P> {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = terminate_child(&mut *active.child, &active.program);
            let _ = cleanup_staging(&active.staging_destination);
        }
    }
}

fn send_interrupt(platform: &mut impl Platform, pid: u32) -> Result<(), CaptureError> {
    let plan = CommandPlan::new("kill").arg("-INT").arg(pid.to_string());
    let output = platform
        .output(&plan)
        .map_err(|source| CaptureError::command_io("kill", source))?;
    if output.success {
        Ok(())
    } else {
        Err(CaptureError::CommandFailed {
            program: "kill".to_string(),
            code: output.code,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn wait_for_exit(
    platform: &mut impl Platform,
    child: &mut dyn CaptureChild,
    program: &str,
) -> Result<(), CaptureError> {
    let mut waited = Duration::ZERO;
    while waited < STOP_TIMEOUT {
        if child
            .try_wait()
            .map_err(|source| CaptureError::command_io(program, source))?
            .is_some()
        {
            return Ok(());
        }
        platform.sleep(STOP_POLL_INTERVAL);
        waited += STOP_POLL_INTERVAL;
    }
    Err(CaptureError::StopTimedOut)
}

fn terminate_child(child: &mut dyn CaptureChild, program: &str) -> Result<(), CaptureError> {
    if child
        .try_wait()
        .map_err(|source| CaptureError::command_io(program, source))?
        .is_none()
    {
        child
            .kill()
            .map_err(|source| CaptureError::command_io(program, source))?;
        let _exit = child
            .wait()
            .map_err(|source| CaptureError::command_io(program, source))?;
    }
    Ok(())
}

fn promote_staging(staging: &Path, destination: &Path) -> Result<(), CaptureError> {
    let staging_file = File::open(staging).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            CaptureError::OutputMissing(staging.display().to_string())
        } else {
            finalize_io("open capture staging file", staging, source)
        }
    })?;
    let metadata = staging_file
        .metadata()
        .map_err(|source| finalize_io("inspect capture staging file", staging, source))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(CaptureError::OutputMissing(staging.display().to_string()));
    }
    staging_file
        .sync_all()
        .map_err(|source| finalize_io("sync capture staging file", staging, source))?;
    fs::hard_link(staging, destination).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            CaptureError::DestinationExists(destination.display().to_string())
        } else {
            finalize_io("promote capture staging file", destination, source)
        }
    })?;
    fs::remove_file(staging)
        .map_err(|source| finalize_io("remove promoted staging link", staging, source))?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| finalize_io("sync recording directory", parent, source))?;
    Ok(())
}

fn cleanup_staging(path: &Path) -> Result<(), CaptureError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(finalize_io("remove capture staging file", path, source)),
    }
}

fn finalize_io(action: &'static str, path: &Path, source: io::Error) -> CaptureError {
    CaptureError::FinalizeIo {
        action,
        path: path.display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::{CommandOutput, ProcessExit},
        discovery::{
            AudioSource, AudioSourceKind, CaptureOutput, Geometry, SessionKind, ToolCapabilities,
        },
        plan::AudioSelection,
    };
    use std::{
        collections::VecDeque,
        ffi::OsStr,
        fs, io,
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct ChildState {
        waits: VecDeque<Option<ProcessExit>>,
        killed: bool,
        waited: bool,
    }

    struct MockChild {
        state: Arc<Mutex<ChildState>>,
    }

    impl CaptureChild for MockChild {
        fn id(&self) -> u32 {
            4242
        }

        fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
            Ok(self.state.lock().unwrap().waits.pop_front().unwrap_or(None))
        }

        fn kill(&mut self) -> io::Result<()> {
            self.state.lock().unwrap().killed = true;
            Ok(())
        }

        fn wait(&mut self) -> io::Result<ProcessExit> {
            self.state.lock().unwrap().waited = true;
            Ok(ProcessExit {
                success: false,
                code: None,
            })
        }
    }

    struct MockPlatform {
        child: Option<Box<dyn CaptureChild>>,
        signal: CommandOutput,
        spawned: Vec<CommandPlan>,
        signalled: Vec<CommandPlan>,
        sleeps: usize,
    }

    impl Platform for MockPlatform {
        fn executable_exists(&self, _: &OsStr) -> bool {
            true
        }

        fn output(&mut self, plan: &CommandPlan) -> io::Result<CommandOutput> {
            self.signalled.push(plan.clone());
            Ok(self.signal.clone())
        }

        fn spawn(&mut self, plan: &CommandPlan) -> io::Result<Box<dyn CaptureChild>> {
            self.spawned.push(plan.clone());
            self.child
                .take()
                .ok_or_else(|| io::Error::other("no child"))
        }

        fn sleep(&mut self, _: Duration) {
            self.sleeps += 1;
        }
    }

    fn fixture() -> (CaptureCapabilities, CaptureConfig, PathBuf) {
        let output = CaptureOutput {
            name: "DP-1".into(),
            description: "Main".into(),
            geometry: Geometry {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            scale: 1.0,
            pixel_size: (100, 100),
            transform: crate::discovery::OutputTransform::Normal,
            refresh_hz: 60.0,
            focused: true,
        };
        let capabilities = CaptureCapabilities {
            session: SessionKind::HyprlandWayland,
            tools: ToolCapabilities {
                gpu_screen_recorder: true,
                wf_recorder: true,
                hyprctl: true,
                pactl: true,
                pw_dump: true,
                kill: true,
            },
            outputs: vec![output.clone()],
            audio_sources: vec![AudioSource {
                name: "mic".into(),
                description: "Mic".into(),
                kind: AudioSourceKind::Microphone,
                is_default: true,
                state: None,
            }],
        };
        let path = std::env::temp_dir().join(format!(
            "dicta-capture-test-{}-{}.mp4",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let config = CaptureConfig::new(&output, AudioSelection::None, &path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&config.staging_destination);
        (capabilities, config, path)
    }

    fn platform(
        waits: impl IntoIterator<Item = Option<ProcessExit>>,
    ) -> (MockPlatform, Arc<Mutex<ChildState>>) {
        let state = Arc::new(Mutex::new(ChildState {
            waits: waits.into_iter().collect(),
            ..ChildState::default()
        }));
        (
            MockPlatform {
                child: Some(Box::new(MockChild {
                    state: Arc::clone(&state),
                })),
                signal: CommandOutput::success(Vec::new()),
                spawned: Vec::new(),
                signalled: Vec::new(),
                sleeps: 0,
            },
            state,
        )
    }

    #[test]
    fn deadline_poll_stops_at_twenty_minutes_with_sigint() {
        let (capabilities, config, path) = fixture();
        let (platform, state) = platform([
            None,
            None,
            Some(ProcessExit {
                success: true,
                code: Some(0),
            }),
        ]);
        let start = Instant::now();
        let mut recorder = Recorder::new(platform);
        recorder.start_at(&capabilities, &config, start).unwrap();
        fs::write(&config.staging_destination, b"video").unwrap();
        let outcome = recorder.poll_at(start + MAX_RECORDING_DURATION).unwrap();
        assert!(matches!(
            outcome,
            PollOutcome::Stopped {
                reason: StopReason::Deadline,
                ..
            }
        ));
        assert_eq!(
            recorder.platform().signalled[0].arguments(),
            ["-INT", "4242"].map(std::ffi::OsString::from)
        );
        assert!(!state.lock().unwrap().killed);
        assert_eq!(fs::read(&path).unwrap(), b"video");
        assert!(!config.staging_destination.exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn recorder_exposes_selected_backend_and_uses_wf_fallback() {
        let (mut capabilities, config, _) = fixture();
        let (gpu_platform, _) = platform([None, None]);
        let mut recorder = Recorder::new(gpu_platform);
        recorder.start(&capabilities, &config).unwrap();
        assert_eq!(
            recorder.active_backend(),
            Some(CaptureBackend::GpuScreenRecorder)
        );
        assert_eq!(
            recorder.platform().spawned[0].program(),
            "gpu-screen-recorder"
        );
        recorder.abort().unwrap();

        capabilities.tools.gpu_screen_recorder = false;
        let (wf_platform, _) = platform([None, None]);
        let mut recorder = Recorder::new(wf_platform);
        recorder.start(&capabilities, &config).unwrap();
        assert_eq!(recorder.active_backend(), Some(CaptureBackend::WfRecorder));
        assert_eq!(recorder.platform().spawned[0].program(), "wf-recorder");
        recorder.abort().unwrap();
    }

    #[test]
    fn stop_timeout_forces_child_cleanup() {
        let (capabilities, config, path) = fixture();
        let (platform, state) = platform(std::iter::repeat_n(None, 200));
        let mut recorder = Recorder::new(platform);
        recorder.start(&capabilities, &config).unwrap();
        fs::write(&config.staging_destination, b"partial").unwrap();
        assert!(matches!(recorder.stop(), Err(CaptureError::StopTimedOut)));
        let state = state.lock().unwrap();
        assert!(state.killed);
        assert!(state.waited);
        assert!(!recorder.is_recording());
        assert!(!config.staging_destination.exists());
        assert!(!path.exists());
    }

    #[test]
    fn dropping_an_active_recorder_reaps_the_child() {
        let (capabilities, config, _) = fixture();
        let (platform, state) = platform([None, None]);
        let mut recorder = Recorder::new(platform);
        recorder.start(&capabilities, &config).unwrap();
        fs::write(&config.staging_destination, b"partial").unwrap();
        drop(recorder);
        let state = state.lock().unwrap();
        assert!(state.killed);
        assert!(state.waited);
        assert!(!config.staging_destination.exists());
    }

    #[test]
    fn finalization_never_replaces_an_existing_destination() {
        let (capabilities, config, path) = fixture();
        let (platform, _) = platform([
            None,
            Some(ProcessExit {
                success: true,
                code: Some(0),
            }),
        ]);
        let mut recorder = Recorder::new(platform);
        recorder.start(&capabilities, &config).unwrap();
        fs::write(&config.staging_destination, b"new recording").unwrap();
        fs::write(&path, b"existing recording").unwrap();

        assert!(matches!(
            recorder.stop(),
            Err(CaptureError::DestinationExists(existing)) if existing == path.display().to_string()
        ));
        assert_eq!(fs::read(&path).unwrap(), b"existing recording");
        assert!(!config.staging_destination.exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn abort_removes_staging_but_preserves_a_concurrent_final_file() {
        let (capabilities, config, path) = fixture();
        let (platform, state) = platform([None, None]);
        let mut recorder = Recorder::new(platform);
        recorder.start(&capabilities, &config).unwrap();
        fs::write(&config.staging_destination, b"partial").unwrap();
        fs::write(&path, b"someone else's file").unwrap();

        recorder.abort().unwrap();
        assert!(!config.staging_destination.exists());
        assert_eq!(fs::read(&path).unwrap(), b"someone else's file");
        assert!(state.lock().unwrap().killed);
        let _ = fs::remove_file(path);
    }
}
