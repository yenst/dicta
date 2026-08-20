//! Bounded synchronous Unix-socket service for the native runtime.

use crate::{
    AnnotationPort, CapturePort, Clock, IdSource, Runtime, RuntimeError, RuntimeSnapshot,
    StoragePort, TranscriptionPort,
};
use dicta_control::{
    socket::{validate_private_socket, ControlError, LocalServer, RequestPoll},
    EventEnvelope,
};
use std::{
    error::Error,
    fmt, fs, io,
    num::NonZeroUsize,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub const DEFAULT_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(25);
pub const MAX_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceConfig {
    pub max_requests_per_connection: NonZeroUsize,
    pub idle_poll_interval: Duration,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            max_requests_per_connection: NonZeroUsize::new(256).unwrap_or(NonZeroUsize::MIN),
            idle_poll_interval: DEFAULT_IDLE_POLL_INTERVAL,
        }
    }
}

/// Cloneable stop signal for a service runner. It owns no thread or global state.
#[derive(Clone, Debug, Default)]
pub struct ShutdownHandle {
    requested: Arc<AtomicBool>,
}

impl ShutdownHandle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionEnd {
    ClientClosed,
    RequestLimitReached,
    ShutdownRequested,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServeReport {
    pub requests_served: usize,
    pub end: ConnectionEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunReport {
    pub connections_served: usize,
    pub requests_served: usize,
}

#[derive(Debug)]
pub enum ServiceError {
    Control(ControlError),
    LiveSocket(PathBuf),
    UnsafeSocket(String),
    InvalidConfig(String),
    Runtime(RuntimeError),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(error) => error.fmt(formatter),
            Self::LiveSocket(path) => {
                write!(
                    formatter,
                    "Dicta is already listening on {}",
                    path.display()
                )
            }
            Self::UnsafeSocket(message) => {
                write!(formatter, "refusing unsafe socket cleanup: {message}")
            }
            Self::InvalidConfig(message) => write!(formatter, "invalid service config: {message}"),
            Self::Runtime(error) => error.fmt(formatter),
        }
    }
}

impl Error for ServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Control(error) => Some(error),
            Self::Runtime(error) => Some(error),
            Self::LiveSocket(_) | Self::UnsafeSocket(_) | Self::InvalidConfig(_) => None,
        }
    }
}

impl From<ControlError> for ServiceError {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}

impl From<RuntimeError> for ServiceError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

/// One-runtime, one-listener synchronous service.
///
/// The runtime stays alive across connections, so its event sequence is global
/// for the lifetime of the service rather than restarting per client.
pub struct LocalRuntimeService<C, T, A, S, K, I> {
    server: LocalServer,
    runtime: Runtime<C, T, A, S, K, I>,
    config: ServiceConfig,
}

impl<C, T, A, S, K, I> LocalRuntimeService<C, T, A, S, K, I>
where
    C: CapturePort,
    T: TranscriptionPort,
    A: AnnotationPort,
    S: StoragePort,
    K: Clock,
    I: IdSource,
{
    /// Binds after safely rejecting live, foreign, permissive, or non-socket paths.
    /// A connection-refused socket is removed only after its device/inode identity
    /// is rechecked, proving the path did not change during the liveness probe.
    ///
    /// # Errors
    /// Returns a security error for an unsafe path, [`ServiceError::LiveSocket`]
    /// when a listener answers, or a control I/O error when binding fails.
    pub fn bind(
        path: impl AsRef<Path>,
        runtime: Runtime<C, T, A, S, K, I>,
        config: ServiceConfig,
    ) -> Result<Self, ServiceError> {
        let path = path.as_ref();
        validate_config(config)?;
        remove_proven_stale_socket(path)?;
        let server = LocalServer::bind(path)?;
        Ok(Self {
            server,
            runtime,
            config,
        })
    }

    #[must_use]
    pub fn socket_path(&self) -> &Path {
        self.server.path()
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.runtime.snapshot()
    }

    /// Accepts one client and serves a bounded number of newline-delimited
    /// requests. Runtime events are written before their correlated response so
    /// existing clients can buffer them without losing request correlation.
    ///
    /// # Errors
    /// Returns a typed control error when accepting, decoding, or writing fails.
    pub fn serve_one_connection(&mut self) -> Result<ServeReport, ServiceError> {
        let connection = self.server.accept()?;
        self.serve_connection(connection, &mut |_| {}, None)
    }

    /// Runs until the supplied handle requests shutdown. Accept polling is
    /// bounded by [`ServiceConfig::idle_poll_interval`], and consuming `self`
    /// guarantees listener and identity-guarded socket cleanup before return.
    ///
    /// Active client reads remain blocking in this first slice; the stop bound
    /// applies while the service has no connected client.
    ///
    /// # Errors
    /// Returns a typed control error when listener configuration, accepting,
    /// decoding, or writing fails. The socket is still cleaned on every exit.
    pub fn run_until_shutdown(self, shutdown: &ShutdownHandle) -> Result<RunReport, ServiceError> {
        self.run_until_shutdown_with_observer(shutdown, |_| {})
    }

    /// Runs with a callback for newly emitted runtime events. Replayed events
    /// from an `events` query are not observed a second time. This keeps native
    /// hosts aligned with capture transitions without coupling the service to Qt.
    ///
    /// # Errors
    /// Returns the same failures as [`Self::run_until_shutdown`].
    pub fn run_until_shutdown_with_observer<F>(
        mut self,
        shutdown: &ShutdownHandle,
        mut observer: F,
    ) -> Result<RunReport, ServiceError>
    where
        F: FnMut(&EventEnvelope),
    {
        self.server.set_nonblocking(true)?;
        let mut report = RunReport {
            connections_served: 0,
            requests_served: 0,
        };
        while !shutdown.is_requested() {
            self.observe_background(&mut observer)?;
            if let Some(connection) = self.server.try_accept()? {
                let connection_report =
                    self.serve_connection(connection, &mut observer, Some(shutdown))?;
                report.connections_served = report.connections_served.saturating_add(1);
                report.requests_served = report
                    .requests_served
                    .saturating_add(connection_report.requests_served);
            } else {
                thread::sleep(self.config.idle_poll_interval);
            }
        }
        Ok(report)
    }

    fn observe_background<F>(
        &mut self,
        observer: &mut F,
    ) -> Result<Vec<EventEnvelope>, ServiceError>
    where
        F: FnMut(&EventEnvelope),
    {
        let previous_sequence = self.runtime.snapshot().last_event_sequence;
        self.runtime.poll_background()?;
        let events = self.runtime.events_since(Some(previous_sequence));
        for event in &events {
            observer(event);
        }
        Ok(events)
    }

    fn serve_connection<F>(
        &mut self,
        mut connection: dicta_control::socket::ServerConnection,
        observer: &mut F,
        shutdown: Option<&ShutdownHandle>,
    ) -> Result<ServeReport, ServiceError>
    where
        F: FnMut(&EventEnvelope),
    {
        let limit = self.config.max_requests_per_connection.get();
        for request_index in 0..limit {
            let request = loop {
                if let Some(shutdown) = shutdown {
                    if shutdown.is_requested() {
                        return Ok(ServeReport {
                            requests_served: request_index,
                            end: ConnectionEnd::ShutdownRequested,
                        });
                    }
                    for event in self.observe_background(observer)? {
                        connection.send_event(&event)?;
                    }
                    match connection.poll_request()? {
                        RequestPoll::Request(request) => break request,
                        RequestPoll::Pending => thread::sleep(self.config.idle_poll_interval),
                        RequestPoll::Closed => {
                            return Ok(ServeReport {
                                requests_served: request_index,
                                end: ConnectionEnd::ClientClosed,
                            });
                        }
                    }
                } else if let Some(request) = connection.read_request()? {
                    break request;
                } else {
                    return Ok(ServeReport {
                        requests_served: request_index,
                        end: ConnectionEnd::ClientClosed,
                    });
                }
            };
            let previous_sequence = self.runtime.snapshot().last_event_sequence;
            let output = self.runtime.handle(request);
            for event in &output.events {
                if crate::event_sequence(&event.event) > previous_sequence {
                    observer(event);
                }
                connection.send_event(event)?;
            }
            connection.send_response(&output.response)?;
        }
        Ok(ServeReport {
            requests_served: limit,
            end: ConnectionEnd::RequestLimitReached,
        })
    }

    /// Consumes the service and synchronously drops its listener. `LocalServer`
    /// removes the path only if its device/inode identity still matches.
    pub fn shutdown(self) {}
}

fn validate_config(config: ServiceConfig) -> Result<(), ServiceError> {
    if config.idle_poll_interval.is_zero() {
        return Err(ServiceError::InvalidConfig(
            "idle poll interval must be greater than zero".to_owned(),
        ));
    }
    if config.idle_poll_interval > MAX_IDLE_POLL_INTERVAL {
        return Err(ServiceError::InvalidConfig(format!(
            "idle poll interval cannot exceed {} ms",
            MAX_IDLE_POLL_INTERVAL.as_millis()
        )));
    }
    Ok(())
}

fn remove_proven_stale_socket(path: &Path) -> Result<(), ServiceError> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ControlError::Io(error).into()),
    };
    if before.file_type().is_symlink() || !before.file_type().is_socket() {
        return Err(ServiceError::UnsafeSocket(format!(
            "{} is not a real Unix socket",
            path.display()
        )));
    }
    validate_private_socket(path)?;
    let identity = (before.dev(), before.ino());

    match UnixStream::connect(path) {
        Ok(_) => return Err(ServiceError::LiveSocket(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
        Err(error) => return Err(ControlError::Io(error).into()),
    }

    let after = fs::symlink_metadata(path).map_err(ControlError::Io)?;
    if after.file_type().is_symlink()
        || !after.file_type().is_socket()
        || (after.dev(), after.ino()) != identity
    {
        return Err(ServiceError::UnsafeSocket(format!(
            "{} changed during its liveness probe",
            path.display()
        )));
    }
    validate_private_socket(path)?;
    fs::remove_file(path).map_err(ControlError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Completion, PortError, PortErrorKind, RuntimeConfig};
    use dicta_capture::CaptureArtifact;
    use dicta_control::{
        socket::{LocalClient, LocalServer},
        AnnotationTool, Command, Event, Response,
    };
    use dicta_core::{AnnotationFile, RecordingFile, RecordingId};
    use dicta_engine::{RecordingSession, StateKind};
    use dicta_transcribe::TranscriptionOutput;
    use std::{
        fs,
        os::unix::{
            fs::{symlink, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
        sync::mpsc,
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    struct SocketCapture;

    impl CapturePort for SocketCapture {
        fn start(&mut self, _session: &RecordingSession) -> Result<Completion<()>, PortError> {
            Ok(Completion::Ready(()))
        }

        fn stop(
            &mut self,
            _session: &RecordingSession,
        ) -> Result<Completion<CaptureArtifact>, PortError> {
            Ok(Completion::Pending)
        }
    }

    struct SocketTranscription;

    impl TranscriptionPort for SocketTranscription {
        fn transcribe(
            &mut self,
            _recording: &RecordingFile,
        ) -> Result<Completion<TranscriptionOutput>, PortError> {
            Ok(Completion::Pending)
        }
    }

    struct SocketAnnotations;

    impl AnnotationPort for SocketAnnotations {
        fn set_enabled(
            &mut self,
            _recording_id: &RecordingId,
            _enabled: bool,
        ) -> Result<(), PortError> {
            Ok(())
        }

        fn set_tool(
            &mut self,
            _recording_id: &RecordingId,
            _tool: AnnotationTool,
        ) -> Result<(), PortError> {
            Ok(())
        }

        fn undo(&mut self, _recording_id: &RecordingId) -> Result<(), PortError> {
            Ok(())
        }

        fn clear(&mut self, _recording_id: &RecordingId) -> Result<(), PortError> {
            Ok(())
        }

        fn finish(
            &mut self,
            _recording_id: &RecordingId,
        ) -> Result<Option<AnnotationFile>, PortError> {
            Ok(None)
        }
    }

    struct SocketStorage;

    impl StoragePort for SocketStorage {
        fn save_recording(
            &mut self,
            _session: &RecordingSession,
            _artifact: &CaptureArtifact,
            _annotations: Option<&AnnotationFile>,
        ) -> Result<RecordingFile, PortError> {
            Err(PortError::new(
                PortErrorKind::Internal,
                "storage is unused in socket tests",
            ))
        }

        fn save_transcription(
            &mut self,
            _recording_id: &RecordingId,
            _output: &TranscriptionOutput,
        ) -> Result<(), PortError> {
            Ok(())
        }
    }

    struct SocketClock;

    impl Clock for SocketClock {
        fn now(&self) -> SystemTime {
            UNIX_EPOCH
        }
    }

    struct SocketIds;

    impl IdSource for SocketIds {
        fn next_recording_id(&mut self, _now: SystemTime) -> Result<RecordingId, PortError> {
            RecordingId::new("socket-recording")
                .map_err(|error| PortError::new(PortErrorKind::Internal, error.to_string()))
        }
    }

    type SocketRuntime = Runtime<
        SocketCapture,
        SocketTranscription,
        SocketAnnotations,
        SocketStorage,
        SocketClock,
        SocketIds,
    >;

    fn runtime() -> SocketRuntime {
        Runtime::new(
            SocketCapture,
            SocketTranscription,
            SocketAnnotations,
            SocketStorage,
            SocketClock,
            SocketIds,
            RuntimeConfig::default(),
        )
    }

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dicta-runtime-service-{name}-{}-{:?}",
            std::process::id(),
            thread::current().id()
        ))
    }

    fn clean_directory(path: &Path) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn service_preserves_sequence_and_correlation_across_connections() {
        let directory = test_directory("round-trip");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        let mut service = LocalRuntimeService::bind(
            &path,
            runtime(),
            ServiceConfig {
                max_requests_per_connection: NonZeroUsize::new(8).unwrap_or(NonZeroUsize::MIN),
                ..ServiceConfig::default()
            },
        )
        .unwrap();

        let server_thread = thread::spawn(move || {
            let first = service.serve_one_connection().unwrap();
            let second = service.serve_one_connection().unwrap();
            assert_eq!(first.requests_served, 2);
            assert_eq!(second.requests_served, 1);
            assert_eq!(service.snapshot().app.state.kind(), StateKind::Annotating);
            service
        });

        let mut first = LocalClient::connect(&path).unwrap();
        let start_id = first
            .send(Command::RecordStart {
                project: None,
                note: None,
            })
            .unwrap();
        let status_id = first.send(Command::Status).unwrap();
        assert!(matches!(
            first.wait(status_id).unwrap(),
            Response::Status(_)
        ));
        assert_eq!(first.wait(start_id).unwrap(), Response::Accepted);
        let mut sequences = Vec::new();
        while let Some(event) = first.pop_event() {
            sequences.push(super::super::event_sequence(&event.event));
        }
        assert_eq!(sequences, vec![1, 2, 3]);
        drop(first);

        let mut second = LocalClient::connect(&path).unwrap();
        assert_eq!(
            second.request(Command::AnnotationEnable).unwrap(),
            Response::Accepted
        );
        let event = second.pop_event().unwrap();
        assert!(matches!(
            event.event,
            Event::StateChanged { sequence: 4, .. }
        ));
        drop(second);

        let service = server_thread.join().unwrap();
        service.shutdown();
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn service_enforces_the_per_connection_request_bound() {
        let directory = test_directory("bound");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        let mut service = LocalRuntimeService::bind(
            &path,
            runtime(),
            ServiceConfig {
                max_requests_per_connection: NonZeroUsize::MIN,
                ..ServiceConfig::default()
            },
        )
        .unwrap();
        let server_thread = thread::spawn(move || service.serve_one_connection().unwrap());
        let mut client = LocalClient::connect(&path).unwrap();
        assert!(matches!(
            client.request(Command::Status).unwrap(),
            Response::Status(_)
        ));
        let report = server_thread.join().unwrap();
        assert_eq!(report.requests_served, 1);
        assert_eq!(report.end, ConnectionEnd::RequestLimitReached);
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn idle_runner_stops_within_the_configured_poll_bound_and_cleans_socket() {
        let directory = test_directory("idle-shutdown");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        let service = LocalRuntimeService::bind(
            &path,
            runtime(),
            ServiceConfig {
                idle_poll_interval: Duration::from_millis(10),
                ..ServiceConfig::default()
            },
        )
        .unwrap();
        let shutdown = ShutdownHandle::new();
        let runner_shutdown = shutdown.clone();
        let server_thread =
            thread::spawn(move || service.run_until_shutdown(&runner_shutdown).unwrap());

        thread::sleep(Duration::from_millis(30));
        let requested_at = Instant::now();
        shutdown.request();
        let report = server_thread.join().unwrap();
        assert!(requested_at.elapsed() < Duration::from_millis(250));
        assert_eq!(
            report,
            RunReport {
                connections_served: 0,
                requests_served: 0
            }
        );
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn silent_connected_client_cannot_block_shutdown_or_socket_cleanup() {
        let directory = test_directory("silent-client-shutdown");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        let service = LocalRuntimeService::bind(
            &path,
            runtime(),
            ServiceConfig {
                idle_poll_interval: Duration::from_millis(10),
                ..ServiceConfig::default()
            },
        )
        .unwrap();
        let shutdown = ShutdownHandle::new();
        let runner_shutdown = shutdown.clone();
        let server_thread =
            thread::spawn(move || service.run_until_shutdown(&runner_shutdown).unwrap());
        let client = UnixStream::connect(&path).unwrap();
        thread::sleep(Duration::from_millis(30));

        let requested_at = Instant::now();
        shutdown.request();
        let report = server_thread.join().unwrap();
        assert!(requested_at.elapsed() < Duration::from_millis(250));
        assert_eq!(report.connections_served, 1);
        assert_eq!(report.requests_served, 0);
        assert!(!path.exists());
        drop(client);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn runner_observes_new_events_without_replaying_history() {
        let directory = test_directory("observer");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        let service =
            LocalRuntimeService::bind(&path, runtime(), ServiceConfig::default()).unwrap();
        let shutdown = ShutdownHandle::new();
        let runner_shutdown = shutdown.clone();
        let (events_sender, events_receiver) = mpsc::channel();
        let server_thread = thread::spawn(move || {
            service
                .run_until_shutdown_with_observer(&runner_shutdown, move |event| {
                    events_sender
                        .send(crate::event_sequence(&event.event))
                        .unwrap();
                })
                .unwrap()
        });

        let mut client = LocalClient::connect(&path).unwrap();
        assert_eq!(
            client
                .request(Command::RecordStart {
                    project: None,
                    note: None,
                })
                .unwrap(),
            Response::Accepted
        );
        assert_eq!(events_receiver.recv().unwrap(), 1);
        assert_eq!(events_receiver.recv().unwrap(), 2);
        assert_eq!(events_receiver.recv().unwrap(), 3);
        assert_eq!(
            client
                .request(Command::Events {
                    since_sequence: None
                })
                .unwrap(),
            Response::Accepted
        );
        assert!(events_receiver.try_recv().is_err());
        drop(client);
        shutdown.request();
        let report = server_thread.join().unwrap();
        assert_eq!(report.connections_served, 1);
        assert_eq!(report.requests_served, 2);
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn service_can_start_and_stop_repeatedly_on_the_same_path() {
        let directory = test_directory("restart");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        for _ in 0..5 {
            let service =
                LocalRuntimeService::bind(&path, runtime(), ServiceConfig::default()).unwrap();
            let shutdown = ShutdownHandle::new();
            let runner_shutdown = shutdown.clone();
            let server_thread =
                thread::spawn(move || service.run_until_shutdown(&runner_shutdown).unwrap());
            shutdown.request();
            server_thread.join().unwrap();
            assert!(!path.exists());
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn live_socket_is_never_removed_or_rebound() {
        let directory = test_directory("live");
        clean_directory(&directory);
        let path = directory.join("control.sock");
        let live = LocalServer::bind(&path).unwrap();
        let error = LocalRuntimeService::bind(&path, runtime(), ServiceConfig::default())
            .err()
            .unwrap();
        assert!(matches!(error, ServiceError::LiveSocket(_)));
        assert!(path.exists());
        drop(live);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn proven_stale_private_socket_is_replaced_and_cleaned_up() {
        let directory = test_directory("stale");
        clean_directory(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("control.sock");
        let stale = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        drop(stale);

        let service =
            LocalRuntimeService::bind(&path, runtime(), ServiceConfig::default()).unwrap();
        assert!(path.exists());
        service.shutdown();
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn symlink_and_permissive_socket_are_rejected_without_cleanup() {
        let directory = test_directory("unsafe");
        let target_directory = test_directory("unsafe-target");
        clean_directory(&directory);
        clean_directory(&target_directory);
        fs::create_dir_all(&directory).unwrap();
        fs::create_dir_all(&target_directory).unwrap();
        let target = target_directory.join("target.sock");
        let listener = UnixListener::bind(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let link = directory.join("link.sock");
        symlink(&target, &link).unwrap();
        assert!(matches!(
            LocalRuntimeService::bind(&link, runtime(), ServiceConfig::default()).err(),
            Some(ServiceError::UnsafeSocket(_))
        ));
        assert!(target.exists());
        fs::remove_file(&link).unwrap();
        drop(listener);
        fs::remove_file(&target).unwrap();

        let permissive = directory.join("permissive.sock");
        let listener = UnixListener::bind(&permissive).unwrap();
        fs::set_permissions(&permissive, fs::Permissions::from_mode(0o666)).unwrap();
        drop(listener);
        assert!(
            LocalRuntimeService::bind(&permissive, runtime(), ServiceConfig::default()).is_err()
        );
        assert!(permissive.exists());
        fs::remove_file(permissive).unwrap();
        fs::remove_dir_all(directory).unwrap();
        fs::remove_dir_all(target_directory).unwrap();
    }
}
