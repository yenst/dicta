use crate::LinuxTranscriptionConfig;
use dicta_core::{RecordingFile, RecordingId};
use dicta_runtime::{
    Completion, ModelInstallPoll, PortError, PortErrorKind, TranscriptionCompletion,
    TranscriptionPort,
};
use dicta_transcribe::{
    FailureKind, IdleReleasePolicy, ManagedModelStatus, ModelInstallError, ModelInstallFailure,
    ModelInstallOutcome, ModelInstaller, ModelPreparation, ModelStatus, ProcessExecutor,
    SystemProcessExecutor, TranscriptionError, TranscriptionEvent, TranscriptionOutput,
    TranscriptionRequest, TranscriptionWorker, VoxtypeBackendFactory, WorkerConfig,
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug)]
pub struct DisabledTranscriptionPort {
    reason: String,
}

impl DisabledTranscriptionPort {
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl Default for DisabledTranscriptionPort {
    fn default() -> Self {
        Self::new("local transcription is disabled")
    }
}

impl TranscriptionPort for DisabledTranscriptionPort {
    fn transcribe(
        &mut self,
        _recording: &RecordingFile,
    ) -> Result<Completion<TranscriptionOutput>, PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            self.reason.clone(),
        ))
    }
}

pub struct LinuxTranscriptionPort {
    state: TranscriptionState,
    config: LinuxTranscriptionConfig,
    executor: Arc<dyn ProcessExecutor>,
    installer: ModelInstaller,
    install_task: Option<ModelInstallTask>,
    last_install_error: Option<String>,
    worker_fingerprint: WorkerFingerprint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkerFingerprint {
    enabled: bool,
    backend_available: bool,
    model_kind: Option<dicta_transcribe::ModelKind>,
    model_identity: Option<(PathBuf, u64, Option<SystemTime>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinuxModelInstallEvent {
    Progress(ModelPreparation),
    Completed(Result<ModelInstallOutcome, PortError>),
}

enum TranscriptionState {
    Disabled(DisabledTranscriptionPort),
    Local {
        worker: TranscriptionWorker,
        language: dicta_transcribe::Language,
        model: dicta_transcribe::ModelSelection,
        pending: Option<(dicta_transcribe::JobId, RecordingId)>,
    },
}

impl LinuxTranscriptionPort {
    pub(crate) fn from_config(config: LinuxTranscriptionConfig) -> Result<Self, String> {
        Self::from_config_with_executor(config, Arc::new(SystemProcessExecutor))
    }

    fn from_config_with_executor(
        config: LinuxTranscriptionConfig,
        executor: Arc<dyn ProcessExecutor>,
    ) -> Result<Self, String> {
        let installer = ModelInstaller::new(
            config.catalog.clone(),
            config.installer.clone(),
            Arc::clone(&executor),
        );
        let state = build_transcription_state(&config, &executor, &installer)?;
        let worker_fingerprint = worker_fingerprint(&config, &executor, &installer);
        Ok(Self {
            state,
            config,
            executor,
            installer,
            install_task: None,
            last_install_error: None,
            worker_fingerprint,
        })
    }

    /// Verifies managed quality-model integrity on first explicit request and
    /// then reuses its path/size/mtime identity for inexpensive later status.
    #[must_use]
    pub fn model_status(&mut self) -> ModelStatus {
        let mut status = self.installer.status();
        status.install_progress = self
            .install_task
            .as_ref()
            .and_then(ModelInstallTask::current_progress);
        status.install_error.clone_from(&self.last_install_error);
        self.refresh_worker_if_idle();
        status
    }

    /// Starts a cancellable background installation of the managed quality
    /// model. The curl child never runs on the runtime/control-service thread.
    ///
    /// # Errors
    /// Returns unavailable when an installation or transcription is active, or
    /// an internal error if the bounded installer thread cannot be created.
    pub fn install_quality_model(&mut self) -> Result<Completion<ModelInstallOutcome>, PortError> {
        if self.install_task.is_some() {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "quality model installation is already in progress",
            ));
        }
        if matches!(
            &self.state,
            TranscriptionState::Local {
                pending: Some(_),
                ..
            }
        ) {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "cannot install a model while transcription is in progress",
            ));
        }
        self.last_install_error = None;
        self.install_task = Some(ModelInstallTask::spawn(self.installer.clone()).map_err(
            |error| {
                PortError::new(
                    PortErrorKind::Internal,
                    format!("could not start model installer: {error}"),
                )
            },
        )?);
        Ok(Completion::Pending)
    }

    /// Polls the latest installer progress or terminal outcome without waiting.
    #[must_use]
    pub fn poll_model_install(&mut self) -> Option<LinuxModelInstallEvent> {
        let event = self.install_task.as_mut()?.poll()?;
        if let LinuxModelInstallEvent::Completed(result) = &event {
            if result.is_ok() {
                self.refresh_worker_if_idle();
                self.last_install_error = None;
            } else if let Err(error) = result {
                self.last_install_error = Some(error.message.clone());
            }
            if let Some(mut completed) = self.install_task.take() {
                completed.detach_finished();
            }
        }
        Some(event)
    }

    #[must_use]
    pub fn quality_model_status(&mut self) -> ManagedModelStatus {
        self.model_status().quality
    }

    fn refresh_worker_if_idle(&mut self) {
        let can_replace = match &self.state {
            TranscriptionState::Disabled(_) => true,
            TranscriptionState::Local { pending, .. } => pending.is_none(),
        };
        if !can_replace {
            return;
        }
        let fingerprint = worker_fingerprint(&self.config, &self.executor, &self.installer);
        if fingerprint == self.worker_fingerprint {
            return;
        }
        if let Ok(state) = build_transcription_state(&self.config, &self.executor, &self.installer)
        {
            self.state = state;
            self.worker_fingerprint = fingerprint;
        }
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self.state, TranscriptionState::Local { .. })
    }

    #[must_use]
    pub fn disabled_reason(&self) -> Option<&str> {
        match &self.state {
            TranscriptionState::Disabled(port) => Some(port.reason()),
            TranscriptionState::Local { .. } => None,
        }
    }
}

fn worker_fingerprint(
    config: &LinuxTranscriptionConfig,
    executor: &Arc<dyn ProcessExecutor>,
    installer: &ModelInstaller,
) -> WorkerFingerprint {
    let factory = VoxtypeBackendFactory::new(config.backend.clone(), Arc::clone(executor));
    let model = installer.trusted_provider().available_model(&config.model);
    let model_identity = model.as_ref().and_then(|model| {
        let metadata = fs::metadata(&model.path).ok()?;
        Some((model.path.clone(), metadata.len(), metadata.modified().ok()))
    });
    WorkerFingerprint {
        enabled: config.enabled,
        backend_available: factory.is_available(),
        model_kind: model.as_ref().map(|model| model.kind),
        model_identity,
    }
}

fn build_transcription_state(
    config: &LinuxTranscriptionConfig,
    executor: &Arc<dyn ProcessExecutor>,
    installer: &ModelInstaller,
) -> Result<TranscriptionState, String> {
    if !config.enabled {
        return Ok(TranscriptionState::Disabled(
            DisabledTranscriptionPort::new("local transcription was disabled by configuration"),
        ));
    }
    let provider = installer.trusted_provider();
    if provider.available_model(&config.model).is_none() {
        return Ok(TranscriptionState::Disabled(DisabledTranscriptionPort::new(
            "local transcription is unavailable because no trusted Dicta Whisper model was found",
        )));
    }
    let factory = VoxtypeBackendFactory::new(config.backend.clone(), Arc::clone(executor));
    if !factory.is_available() {
        return Ok(TranscriptionState::Disabled(
            DisabledTranscriptionPort::new(
                "local transcription is unavailable because ffmpeg is missing",
            ),
        ));
    }
    let worker = TranscriptionWorker::spawn(
        WorkerConfig {
            queue_capacity: 1,
            idle_release: IdleReleasePolicy::After(Duration::from_mins(5)),
        },
        provider,
        factory,
    )
    .map_err(|error| format!("could not start the transcription worker: {error}"))?;
    Ok(TranscriptionState::Local {
        worker,
        language: config.language,
        model: config.model.clone(),
        pending: None,
    })
}

#[derive(Default)]
struct ModelInstallShared {
    progress: Option<ModelPreparation>,
    progress_revision: u64,
    result: Option<Result<ModelInstallOutcome, ModelInstallError>>,
}

struct ModelInstallTask {
    shared: Arc<Mutex<ModelInstallShared>>,
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    seen_progress_revision: u64,
}

impl ModelInstallTask {
    fn spawn(installer: ModelInstaller) -> std::io::Result<Self> {
        let shared = Arc::new(Mutex::new(ModelInstallShared::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_shared = Arc::clone(&shared);
        let task_cancelled = Arc::clone(&cancelled);
        let thread = thread::Builder::new()
            .name("dicta-model-install".to_owned())
            .spawn(move || {
                let result = installer.install_quality(&mut |progress| {
                    if task_cancelled.load(Ordering::Acquire) {
                        return false;
                    }
                    let mut shared = task_shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    shared.progress = Some(progress);
                    shared.progress_revision = shared.progress_revision.saturating_add(1);
                    true
                });
                let mut shared = task_shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                shared.progress = None;
                shared.result = Some(result);
            })?;
        Ok(Self {
            shared,
            cancelled,
            thread: Some(thread),
            seen_progress_revision: 0,
        })
    }

    fn poll(&mut self) -> Option<LinuxModelInstallEvent> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(result) = shared.result.take() {
            return Some(LinuxModelInstallEvent::Completed(
                result.map_err(map_model_install_error),
            ));
        }
        if shared.progress_revision > self.seen_progress_revision {
            self.seen_progress_revision = shared.progress_revision;
            return shared
                .progress
                .clone()
                .map(LinuxModelInstallEvent::Progress);
        }
        None
    }

    fn current_progress(&self) -> Option<ModelPreparation> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .progress
            .clone()
    }

    fn detach_finished(&mut self) {
        let _ = self.thread.take();
    }
}

impl Drop for ModelInstallTask {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn map_model_install_error(error: ModelInstallError) -> PortError {
    let kind = match error.kind {
        ModelInstallFailure::Cancelled
        | ModelInstallFailure::ToolUnavailable
        | ModelInstallFailure::Download => PortErrorKind::Unavailable,
        ModelInstallFailure::UnsafePath => PortErrorKind::PermissionDenied,
        ModelInstallFailure::Integrity | ModelInstallFailure::Io => PortErrorKind::Internal,
    };
    PortError::new(kind, error.message)
}

impl TranscriptionPort for LinuxTranscriptionPort {
    fn is_available(&self) -> bool {
        self.is_enabled() && self.install_task.is_none()
    }

    fn set_language(&mut self, language: &str) -> Result<(), PortError> {
        let language = dicta_transcribe::Language::from_code(language).ok_or_else(|| {
            PortError::new(
                PortErrorKind::InvalidRequest,
                format!("unsupported transcription language `{language}`"),
            )
        })?;
        self.config.language = language;
        if let TranscriptionState::Local {
            language: active, ..
        } = &mut self.state
        {
            *active = language;
        }
        Ok(())
    }

    fn transcribe(
        &mut self,
        recording: &RecordingFile,
    ) -> Result<Completion<TranscriptionOutput>, PortError> {
        let TranscriptionState::Local {
            worker,
            language,
            model,
            pending,
        } = &mut self.state
        else {
            let TranscriptionState::Disabled(port) = &mut self.state else {
                unreachable!();
            };
            return port.transcribe(recording);
        };
        if pending.is_some() {
            return Err(PortError::new(
                PortErrorKind::Unavailable,
                "a local transcription is already in progress",
            ));
        }
        let mut request =
            TranscriptionRequest::new(recording.id.clone(), PathBuf::from(&recording.video_path));
        request.language = *language;
        request.model.clone_from(model);
        let job_id = worker.try_submit(request).map_err(|error| {
            PortError::new(
                PortErrorKind::Unavailable,
                format!("could not queue local transcription: {error}"),
            )
        })?;
        *pending = Some((job_id, recording.id.clone()));
        Ok(Completion::Pending)
    }

    fn poll_completion(&mut self) -> Option<TranscriptionCompletion> {
        let TranscriptionState::Local {
            worker, pending, ..
        } = &mut self.state
        else {
            return None;
        };
        // Progress can be bursty. Consume a bounded number of queued events so
        // polling remains strictly nonblocking for the control-service thread.
        for _ in 0..64 {
            match worker.events().try_recv() {
                Ok(TranscriptionEvent::Succeeded {
                    job_id: completed,
                    recording_id,
                    output,
                    ..
                }) => {
                    return finish_completion(pending, completed, &recording_id, Ok(output));
                }
                Ok(TranscriptionEvent::Failed {
                    job_id: completed,
                    recording_id,
                    error,
                    ..
                }) => {
                    return finish_completion(
                        pending,
                        completed,
                        &recording_id,
                        Err(map_transcription_error(error)),
                    );
                }
                Ok(TranscriptionEvent::Stopped) => {
                    let (_, recording_id) = pending.take()?;
                    return Some(TranscriptionCompletion {
                        recording_id,
                        result: Err(PortError::new(
                            PortErrorKind::Internal,
                            "local transcription worker stopped before completing the recording",
                        )),
                    });
                }
                Ok(_) => {}
                Err(error) => return poll_worker_disconnection(pending, error),
            }
        }
        None
    }

    fn model_status(&mut self) -> Result<ModelStatus, PortError> {
        Ok(LinuxTranscriptionPort::model_status(self))
    }

    fn install_quality_model(&mut self) -> Result<Completion<ModelInstallOutcome>, PortError> {
        LinuxTranscriptionPort::install_quality_model(self)
    }

    fn poll_model_install(&mut self) -> Option<ModelInstallPoll> {
        LinuxTranscriptionPort::poll_model_install(self).map(|event| match event {
            LinuxModelInstallEvent::Progress(progress) => ModelInstallPoll::Progress(progress),
            LinuxModelInstallEvent::Completed(result) => ModelInstallPoll::Completed(result),
        })
    }
}

fn finish_completion(
    pending: &mut Option<(dicta_transcribe::JobId, RecordingId)>,
    completed_job: dicta_transcribe::JobId,
    completed_recording: &RecordingId,
    result: Result<TranscriptionOutput, PortError>,
) -> Option<TranscriptionCompletion> {
    let (expected_job, expected_recording) = pending.take()?;
    let result = if completed_job == expected_job && completed_recording == &expected_recording {
        result
    } else {
        Err(PortError::new(
            PortErrorKind::Internal,
            "local transcription worker returned a mismatched completion",
        ))
    };
    Some(TranscriptionCompletion {
        recording_id: expected_recording,
        result,
    })
}

fn poll_worker_disconnection(
    pending: &mut Option<(dicta_transcribe::JobId, RecordingId)>,
    error: TryRecvError,
) -> Option<TranscriptionCompletion> {
    match error {
        TryRecvError::Empty => None,
        TryRecvError::Disconnected => {
            let (_, recording_id) = pending.take()?;
            Some(TranscriptionCompletion {
                recording_id,
                result: Err(PortError::new(
                    PortErrorKind::Internal,
                    "local transcription worker stopped unexpectedly",
                )),
            })
        }
    }
}

fn map_transcription_error(error: TranscriptionError) -> PortError {
    let kind = match error.kind {
        FailureKind::ModelUnavailable | FailureKind::BackendLoad => PortErrorKind::Unavailable,
        FailureKind::Input => PortErrorKind::NotFound,
        FailureKind::ModelIntegrity
        | FailureKind::Inference
        | FailureKind::InvalidOutput
        | FailureKind::Internal => PortErrorKind::Internal,
    };
    PortError::new(kind, error.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicta_transcribe::{
        ModelCatalog, ModelSelection, ProcessOutput, ProcessPlan, VoxtypeBackendConfig,
        COMPACT_FILENAME,
    };
    use std::{
        collections::VecDeque,
        fs, io,
        path::Path,
        sync::{atomic::AtomicU64, Mutex},
    };

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(1);

    struct FakeExecutor {
        outputs: Mutex<VecDeque<ProcessOutput>>,
        available: bool,
    }

    impl ProcessExecutor for FakeExecutor {
        fn executable_available(&self, _program: &Path) -> bool {
            self.available
        }

        fn output(&self, _plan: &ProcessPlan) -> io::Result<ProcessOutput> {
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing fake output"))
        }
    }

    fn fixture_root(label: &str) -> PathBuf {
        use std::sync::atomic::Ordering;
        std::env::temp_dir().join(format!(
            "dicta-linux-transcription-{label}-{}-{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn config(root: &Path) -> LinuxTranscriptionConfig {
        LinuxTranscriptionConfig {
            enabled: true,
            language: dicta_transcribe::Language::Dutch,
            model: ModelSelection::Auto,
            catalog: ModelCatalog::new(root.join("models")),
            backend: VoxtypeBackendConfig {
                ffmpeg_program: PathBuf::from("ffmpeg"),
                voxtype_program: PathBuf::from("voxtype"),
                temporary_root: root.join("temporary"),
                timestamped_whisper: false,
            },
            installer: dicta_transcribe::ModelInstallerConfig::default(),
        }
    }

    #[test]
    fn disabled_port_is_explicitly_unavailable() {
        let recording: RecordingFile =
            serde_json::from_str(r#"{"id":"recording-1","project_id":"__unprojected__"}"#).unwrap();
        let error = DisabledTranscriptionPort::default()
            .transcribe(&recording)
            .unwrap_err();
        assert_eq!(error.kind, PortErrorKind::Unavailable);
    }

    #[test]
    fn missing_tools_gracefully_disable_post_recording_work() {
        let root = fixture_root("disabled");
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join(COMPACT_FILENAME), b"model").unwrap();
        let port = LinuxTranscriptionPort::from_config_with_executor(
            config(&root),
            Arc::new(FakeExecutor {
                outputs: Mutex::new(VecDeque::new()),
                available: false,
            }),
        )
        .unwrap();
        assert!(!port.is_enabled());
        assert!(port.disabled_reason().unwrap().contains("missing"));
        fs::remove_file(root.join("models").join(COMPACT_FILENAME)).unwrap();
        fs::remove_dir(root.join("models")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn local_port_enqueues_then_nonblockingly_polls_for_runtime_persistence() {
        let root = fixture_root("enabled");
        fs::create_dir_all(root.join("models")).unwrap();
        fs::create_dir_all(root.join("temporary")).unwrap();
        fs::write(root.join("models").join(COMPACT_FILENAME), b"model").unwrap();
        let video = root.join("recording.mp4");
        fs::write(&video, b"video").unwrap();
        let executor = FakeExecutor {
            outputs: Mutex::new(VecDeque::from([
                ProcessOutput {
                    success: true,
                    code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
                ProcessOutput {
                    success: true,
                    code: Some(0),
                    stdout: b"technische uitleg".to_vec(),
                    stderr: Vec::new(),
                },
            ])),
            available: true,
        };
        let mut port =
            LinuxTranscriptionPort::from_config_with_executor(config(&root), Arc::new(executor))
                .unwrap();
        let recording: RecordingFile = serde_json::from_value(serde_json::json!({
            "id": "recording-1",
            "project_id": "__unprojected__",
            "video_path": video,
        }))
        .unwrap();
        assert!(matches!(
            port.transcribe(&recording).unwrap(),
            Completion::Pending
        ));
        let duplicate = port.transcribe(&recording).unwrap_err();
        assert_eq!(duplicate.kind, PortErrorKind::Unavailable);

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let completion = loop {
            if let Some(completion) = port.poll_completion() {
                break completion;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fake transcription worker did not finish"
            );
            std::thread::yield_now();
        };
        assert_eq!(completion.recording_id, recording.id);
        let output = completion.result.unwrap();
        assert_eq!(output.transcript, "technische uitleg");
        assert_eq!(output.detected_language.as_deref(), Some("nl"));

        drop(port);
        fs::remove_file(video).unwrap();
        fs::remove_file(root.join("models").join(COMPACT_FILENAME)).unwrap();
        fs::remove_dir(root.join("models")).unwrap();
        fs::remove_dir(root.join("temporary")).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
