use crate::{storage::prepare_capture_path, LinuxConfig, LinuxInitError, StorageLayout};
use dicta_capture::{
    discover, CaptureCapabilities, CaptureError, CaptureOutput, Platform, Recorder,
    SessionEnvironment,
};
use dicta_core::RecordingId;
use dicta_engine::RecordingSession;
use dicta_runtime::{CapturePort, Completion, PortError, PortErrorKind};
use std::{fs, io};

pub trait CaptureStartObserver {
    /// Called immediately after the recorder process starts successfully.
    ///
    /// # Errors
    /// Returns a frontend/overlay error when recording-start state cannot be
    /// made visible. The capture is aborted when this hook fails.
    fn recording_started(
        &mut self,
        session: &RecordingSession,
        output: &CaptureOutput,
    ) -> Result<(), PortError>;
}

impl<F> CaptureStartObserver for F
where
    F: FnMut(&RecordingSession, &CaptureOutput) -> Result<(), PortError>,
{
    fn recording_started(
        &mut self,
        session: &RecordingSession,
        output: &CaptureOutput,
    ) -> Result<(), PortError> {
        self(session, output)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopCaptureStartObserver;

impl CaptureStartObserver for NoopCaptureStartObserver {
    fn recording_started(
        &mut self,
        _session: &RecordingSession,
        _output: &CaptureOutput,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

pub struct LinuxCapture<P: Platform, O = NoopCaptureStartObserver> {
    recorder: Recorder<P>,
    capabilities: CaptureCapabilities,
    selected_output: CaptureOutput,
    config: LinuxConfig,
    layout: StorageLayout,
    observer: O,
    active_recording_id: Option<RecordingId>,
}

impl<P, O> LinuxCapture<P, O>
where
    P: Platform,
    O: CaptureStartObserver,
{
    /// Discovers capabilities using the supplied platform and session.
    ///
    /// # Errors
    /// Returns a typed initialization error when configuration, discovery, or
    /// exact output selection fails.
    pub fn discover(
        mut platform: P,
        environment: &SessionEnvironment,
        config: LinuxConfig,
        observer: O,
    ) -> Result<Self, LinuxInitError> {
        config.validate().map_err(LinuxInitError::new)?;
        let capabilities = discover(&mut platform, environment)
            .map_err(|error| LinuxInitError::new(error.to_string()))?;
        Self::from_capabilities(platform, capabilities, config, observer)
    }

    /// Builds from already-discovered capabilities, primarily for deterministic
    /// integration tests and embedders with a shared discovery pass.
    ///
    /// # Errors
    /// Returns a typed initialization error when configuration is invalid or the
    /// configured output is unavailable.
    pub fn from_capabilities(
        platform: P,
        capabilities: CaptureCapabilities,
        config: LinuxConfig,
        observer: O,
    ) -> Result<Self, LinuxInitError> {
        config.validate().map_err(LinuxInitError::new)?;
        let selected_output = capabilities
            .output(config.output_name.trim())
            .cloned()
            .ok_or_else(|| {
                LinuxInitError::new(format!(
                    "configured capture output `{}` was not discovered",
                    config.output_name.trim()
                ))
            })?;
        let layout = StorageLayout::new(config.storage_root.clone());
        Ok(Self {
            recorder: Recorder::new(platform),
            capabilities,
            selected_output,
            config,
            layout,
            observer,
            active_recording_id: None,
        })
    }

    #[must_use]
    pub const fn selected_output(&self) -> &CaptureOutput {
        &self.selected_output
    }

    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.recorder.is_recording()
    }
}

impl<P, O> CapturePort for LinuxCapture<P, O>
where
    P: Platform,
    O: CaptureStartObserver,
{
    fn start(&mut self, session: &RecordingSession) -> Result<Completion<()>, PortError> {
        if let Some(active) = &self.active_recording_id {
            return Err(PortError::new(
                PortErrorKind::Internal,
                format!("recording `{active}` is already active"),
            ));
        }
        let destination = prepare_capture_path(&self.layout, session)?;
        let parent = destination.parent().ok_or_else(|| {
            PortError::new(
                PortErrorKind::Internal,
                "capture destination has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)
            .map_err(|error| io_port_error("create capture directory", &error))?;

        let mut capture = dicta_capture::CaptureConfig::new(
            &self.selected_output,
            self.config.audio.clone(),
            destination,
        );
        capture.area = self.config.area;
        capture.frame_rate = self.config.frame_rate;
        self.recorder
            .start(&self.capabilities, &capture)
            .map_err(|error| capture_port_error(&error))?;
        if let Err(error) = self
            .observer
            .recording_started(session, &self.selected_output)
        {
            let _ = self.recorder.abort();
            return Err(error);
        }
        self.active_recording_id = Some(session.recording_id.clone());
        Ok(Completion::Ready(()))
    }

    fn stop(
        &mut self,
        session: &RecordingSession,
    ) -> Result<Completion<dicta_capture::CaptureArtifact>, PortError> {
        if self.active_recording_id.as_ref() != Some(&session.recording_id) {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                format!("recording `{}` is not active", session.recording_id),
            ));
        }
        self.active_recording_id = None;
        self.recorder
            .stop()
            .map(Completion::Ready)
            .map_err(|error| capture_port_error(&error))
    }
}

fn capture_port_error(error: &CaptureError) -> PortError {
    let kind = match error {
        CaptureError::MissingTool(_)
        | CaptureError::OutputNotFound(_)
        | CaptureError::AudioSourceNotFound(_)
        | CaptureError::MissingMixedSource(_) => PortErrorKind::Unavailable,
        CaptureError::CommandIo { source, .. } | CaptureError::FinalizeIo { source, .. }
            if source.kind() == io::ErrorKind::PermissionDenied =>
        {
            PortErrorKind::PermissionDenied
        }
        CaptureError::OutputMissing(_) => PortErrorKind::NotFound,
        _ => PortErrorKind::Internal,
    };
    PortError::new(kind, error.to_string())
}

fn io_port_error(action: &str, error: &io::Error) -> PortError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => PortErrorKind::PermissionDenied,
        io::ErrorKind::NotFound => PortErrorKind::NotFound,
        _ => PortErrorKind::Internal,
    };
    PortError::new(kind, format!("could not {action}: {error}"))
}
