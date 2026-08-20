//! Concrete synchronous Linux adapters for the Dicta native runtime.

#![forbid(unsafe_code)]

mod capture;
mod clock;
mod config;
mod ids;
mod omarchy;
mod poster;
mod settings;
mod storage;
mod transcription;

pub use capture::{CaptureStartObserver, LinuxCapture, NoopCaptureStartObserver};
pub use clock::SystemClock;
pub use config::{LinuxConfig, LinuxTranscriptionConfig, StorageLayout};
pub use ids::FilesystemIdSource;
pub use settings::SettingsStore;
pub use storage::LinuxStorage;
pub use transcription::{DisabledTranscriptionPort, LinuxTranscriptionPort};

use dicta_capture::{SessionEnvironment, SystemPlatform};
use dicta_runtime::{AnnotationPort, Runtime, RuntimeConfig};
use std::{error::Error, fmt, fs};

pub type LinuxRuntime<A, O = NoopCaptureStartObserver> = Runtime<
    LinuxCapture<SystemPlatform, O>,
    LinuxTranscriptionPort,
    A,
    LinuxStorage<SystemClock>,
    SystemClock,
    FilesystemIdSource,
>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxInitError {
    message: String,
}

impl LinuxInitError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for LinuxInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for LinuxInitError {}

/// Builds the production synchronous Linux runtime. Local transcription is
/// enabled only when configuration, a model, ffmpeg, and Voxtype are available.
///
/// # Errors
/// Returns a typed initialization error when the explicit root is invalid or
/// unwritable, discovery fails, or the configured output is unavailable.
pub fn build_runtime<A>(
    config: LinuxConfig,
    annotations: A,
) -> Result<LinuxRuntime<A>, LinuxInitError>
where
    A: AnnotationPort,
{
    build_runtime_with_observer(config, annotations, NoopCaptureStartObserver)
}

/// Builds the production runtime with a recording-start observer for overlay
/// activation and capture-relative clocks.
///
/// # Errors
/// Returns a typed initialization error when the explicit root is invalid or
/// unwritable, discovery fails, or the configured output is unavailable.
pub fn build_runtime_with_observer<A, O>(
    config: LinuxConfig,
    annotations: A,
    observer: O,
) -> Result<LinuxRuntime<A, O>, LinuxInitError>
where
    A: AnnotationPort,
    O: CaptureStartObserver,
{
    config.validate().map_err(LinuxInitError::new)?;
    fs::create_dir_all(&config.storage_root).map_err(|error| {
        LinuxInitError::new(format!(
            "could not create storage root {}: {error}",
            config.storage_root.display()
        ))
    })?;
    if !config.storage_root.is_dir() {
        return Err(LinuxInitError::new(format!(
            "storage root is not a directory: {}",
            config.storage_root.display()
        )));
    }
    let layout = StorageLayout::new(config.storage_root.clone());
    let transcription = LinuxTranscriptionPort::from_config(config.transcription.clone())
        .map_err(LinuxInitError::new)?;
    let transcribe_after_recording = config.transcription.enabled;
    let capture = LinuxCapture::discover(
        SystemPlatform,
        &SessionEnvironment::current(),
        config,
        observer,
    )?;
    Ok(Runtime::new(
        capture,
        transcription,
        annotations,
        LinuxStorage::system(layout.clone()).with_retry_discovery(),
        SystemClock,
        FilesystemIdSource::new(layout),
        RuntimeConfig {
            transcribe_after_recording,
        },
    ))
}
