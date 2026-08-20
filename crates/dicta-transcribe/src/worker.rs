use crate::{IntegritySpec, ModelPreparation, ModelSelection, PreparedModel};
use dicta_core::{RecordingId, TranscriptSegment};
use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    Auto,
    Dutch,
    English,
    French,
    German,
    Spanish,
}

impl Language {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Dutch => "nl",
            Self::English => "en",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "auto" => Some(Self::Auto),
            "nl" => Some(Self::Dutch),
            "en" => Some(Self::English),
            "fr" => Some(Self::French),
            "de" => Some(Self::German),
            "es" => Some(Self::Spanish),
            _ => None,
        }
    }

    #[must_use]
    pub const fn whisper_language(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            language => Some(language.code()),
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    ModelUnavailable,
    ModelIntegrity,
    BackendLoad,
    Input,
    Inference,
    InvalidOutput,
    Internal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptionError {
    pub kind: FailureKind,
    pub message: String,
    pub retryable: bool,
}

impl TranscriptionError {
    #[must_use]
    pub fn new(kind: FailureKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }
}

impl fmt::Display for TranscriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for TranscriptionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    max_attempts: u32,
    initial_delay: Duration,
    max_delay: Duration,
}

impl RetryPolicy {
    #[must_use]
    pub fn new(max_attempts: u32, initial_delay: Duration, max_delay: Duration) -> Option<Self> {
        (max_attempts > 0).then_some(Self {
            max_attempts,
            initial_delay,
            max_delay: max_delay.max(initial_delay),
        })
    }

    #[must_use]
    pub const fn max_attempts(self) -> u32 {
        self.max_attempts
    }

    fn delay_after(self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(31);
        self.initial_delay
            .saturating_mul(1_u32 << shift)
            .min(self.max_delay)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 2,
            initial_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdleReleasePolicy {
    Never,
    After(Duration),
    AfterEachJob,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerConfig {
    pub queue_capacity: usize,
    pub idle_release: IdleReleasePolicy,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 8,
            idle_release: IdleReleasePolicy::After(Duration::from_secs(5 * 60)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptionRequest {
    pub recording_id: RecordingId,
    pub input_path: PathBuf,
    pub language: Language,
    pub model: ModelSelection,
    pub retry: RetryPolicy,
}

impl TranscriptionRequest {
    #[must_use]
    pub fn new(recording_id: RecordingId, input_path: PathBuf) -> Self {
        Self {
            recording_id,
            input_path,
            language: Language::Auto,
            model: ModelSelection::Auto,
            retry: RetryPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionOutput {
    pub transcript: String,
    pub segments: Vec<TranscriptSegment>,
    pub detected_language: Option<String>,
}

impl TranscriptionOutput {
    #[must_use]
    pub fn new(transcript: String, segments: Vec<TranscriptSegment>) -> Self {
        Self {
            transcript,
            segments,
            detected_language: None,
        }
    }

    fn validate(&self) -> Result<(), TranscriptionError> {
        if self.transcript.trim().is_empty() {
            return Err(TranscriptionError::new(
                FailureKind::InvalidOutput,
                "transcription backend returned an empty transcript",
                false,
            ));
        }
        if !self.segments.iter().all(TranscriptSegment::is_valid) {
            return Err(TranscriptionError::new(
                FailureKind::InvalidOutput,
                "transcription backend returned invalid timed segments",
                false,
            ));
        }
        if self
            .segments
            .windows(2)
            .any(|pair| pair[1].start_seconds < pair[0].start_seconds)
        {
            return Err(TranscriptionError::new(
                FailureKind::InvalidOutput,
                "transcription backend returned unsorted timed segments",
                false,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadProgress {
    pub completed_units: u64,
    pub total_units: Option<u64>,
    pub message: String,
}

impl LoadProgress {
    #[must_use]
    pub fn new(completed_units: u64, total_units: Option<u64>, message: impl Into<String>) -> Self {
        Self {
            completed_units,
            total_units,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Progress {
    PreparingModel(ModelPreparation),
    LoadingBackend(LoadProgress),
    Transcribing {
        completed_units: u64,
        total_units: Option<u64>,
        message: String,
    },
}

pub trait ModelProvider: Send + 'static {
    fn prepare(
        &mut self,
        selection: &ModelSelection,
        progress: &mut dyn FnMut(ModelPreparation),
    ) -> Result<PreparedModel, TranscriptionError>;
}

/// Integrity adapter used by concrete model providers.
///
/// Keeping checksum implementation outside this crate avoids pulling a hashing
/// stack into processes that package a pre-verified model.
pub trait ModelIntegrityVerifier: Send {
    fn verify(
        &mut self,
        path: &Path,
        integrity: &IntegritySpec,
        progress: &mut dyn FnMut(ModelPreparation),
    ) -> Result<(), TranscriptionError>;
}

pub trait BackendFactory: Send + 'static {
    fn load(
        &mut self,
        model: &PreparedModel,
        progress: &mut dyn FnMut(LoadProgress),
    ) -> Result<Box<dyn TranscriptionBackend>, TranscriptionError>;
}

pub trait TranscriptionBackend: Send {
    fn transcribe(
        &mut self,
        input_path: &Path,
        language: Language,
        progress: &mut dyn FnMut(u64, Option<u64>, String),
    ) -> Result<TranscriptionOutput, TranscriptionError>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct JobId(u64);

impl JobId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseReason {
    IdleTimeout,
    AfterJob,
    ModelChanged,
    Retry,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TranscriptionEvent {
    Queued {
        job_id: JobId,
        recording_id: RecordingId,
    },
    Started {
        job_id: JobId,
        recording_id: RecordingId,
        attempt: u32,
    },
    Progress {
        job_id: JobId,
        progress: Progress,
    },
    RetryScheduled {
        job_id: JobId,
        completed_attempt: u32,
        next_attempt: u32,
        delay: Duration,
        error: TranscriptionError,
    },
    Succeeded {
        job_id: JobId,
        recording_id: RecordingId,
        attempts: u32,
        output: TranscriptionOutput,
    },
    Failed {
        job_id: JobId,
        recording_id: RecordingId,
        attempts: u32,
        error: TranscriptionError,
    },
    ModelReleased {
        model: PreparedModel,
        reason: ReleaseReason,
    },
    Stopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    QueueFull,
    WorkerStopped,
}

impl fmt::Display for SubmitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::QueueFull => "transcription queue is full",
            Self::WorkerStopped => "transcription worker has stopped",
        })
    }
}

impl Error for SubmitError {}

#[derive(Clone)]
pub struct TranscriptionQueue {
    commands: SyncSender<WorkerCommand>,
    next_job_id: Arc<AtomicU64>,
}

impl TranscriptionQueue {
    pub fn try_submit(&self, request: TranscriptionRequest) -> Result<JobId, SubmitError> {
        let job_id = JobId(self.next_job_id.fetch_add(1, Ordering::Relaxed));
        match self
            .commands
            .try_send(WorkerCommand::Transcribe(WorkItem { job_id, request }))
        {
            Ok(()) => Ok(job_id),
            Err(TrySendError::Full(_)) => Err(SubmitError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::WorkerStopped),
        }
    }
}

pub struct TranscriptionWorker {
    queue: TranscriptionQueue,
    events: Receiver<TranscriptionEvent>,
    thread: Option<JoinHandle<()>>,
}

impl TranscriptionWorker {
    pub fn spawn(
        config: WorkerConfig,
        model_provider: impl ModelProvider,
        backend_factory: impl BackendFactory,
    ) -> std::io::Result<Self> {
        if config.queue_capacity == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "transcription queue capacity must be greater than zero",
            ));
        }
        if matches!(config.idle_release, IdleReleasePolicy::After(delay) if delay.is_zero()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "idle release duration must be greater than zero",
            ));
        }

        let (commands_tx, commands_rx) = mpsc::sync_channel(config.queue_capacity);
        let (events_tx, events_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("dicta-transcribe".to_owned())
            .spawn(move || {
                run_worker(
                    config,
                    commands_rx,
                    events_tx,
                    Box::new(model_provider),
                    Box::new(backend_factory),
                );
            })?;
        Ok(Self {
            queue: TranscriptionQueue {
                commands: commands_tx,
                next_job_id: Arc::new(AtomicU64::new(1)),
            },
            events: events_rx,
            thread: Some(thread),
        })
    }

    #[must_use]
    pub fn queue(&self) -> TranscriptionQueue {
        self.queue.clone()
    }

    pub fn try_submit(&self, request: TranscriptionRequest) -> Result<JobId, SubmitError> {
        self.queue.try_submit(request)
    }

    #[must_use]
    pub const fn events(&self) -> &Receiver<TranscriptionEvent> {
        &self.events
    }

    pub fn shutdown(mut self) -> thread::Result<()> {
        let _ = self.queue.commands.send(WorkerCommand::Shutdown);
        self.thread.take().map_or(Ok(()), JoinHandle::join)
    }
}

impl Drop for TranscriptionWorker {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let _ = self.queue.commands.send(WorkerCommand::Shutdown);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

enum WorkerCommand {
    Transcribe(WorkItem),
    Shutdown,
}

struct WorkItem {
    job_id: JobId,
    request: TranscriptionRequest,
}

struct LoadedBackend {
    model: PreparedModel,
    backend: Box<dyn TranscriptionBackend>,
}

fn run_worker(
    config: WorkerConfig,
    commands: Receiver<WorkerCommand>,
    events: mpsc::Sender<TranscriptionEvent>,
    mut models: Box<dyn ModelProvider>,
    mut factory: Box<dyn BackendFactory>,
) {
    let mut loaded = None;
    loop {
        let command = match config.idle_release {
            IdleReleasePolicy::Never | IdleReleasePolicy::AfterEachJob => commands.recv().ok(),
            IdleReleasePolicy::After(delay) => match commands.recv_timeout(delay) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => {
                    release_loaded(&events, &mut loaded, ReleaseReason::IdleTimeout);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => None,
            },
        };

        match command {
            Some(WorkerCommand::Transcribe(item)) => {
                process_item(&events, &mut *models, &mut *factory, &mut loaded, item);
                if config.idle_release == IdleReleasePolicy::AfterEachJob {
                    release_loaded(&events, &mut loaded, ReleaseReason::AfterJob);
                }
            }
            Some(WorkerCommand::Shutdown) | None => break,
        }
    }
    release_loaded(&events, &mut loaded, ReleaseReason::Shutdown);
    emit(&events, TranscriptionEvent::Stopped);
}

fn process_item(
    events: &mpsc::Sender<TranscriptionEvent>,
    models: &mut dyn ModelProvider,
    factory: &mut dyn BackendFactory,
    loaded: &mut Option<LoadedBackend>,
    item: WorkItem,
) {
    let WorkItem { job_id, request } = item;
    emit(
        events,
        TranscriptionEvent::Queued {
            job_id,
            recording_id: request.recording_id.clone(),
        },
    );

    for attempt in 1..=request.retry.max_attempts() {
        emit(
            events,
            TranscriptionEvent::Started {
                job_id,
                recording_id: request.recording_id.clone(),
                attempt,
            },
        );

        let result = ensure_backend(events, job_id, &request.model, models, factory, loaded)
            .and_then(|backend| {
                let mut report = |completed_units, total_units, message| {
                    emit(
                        events,
                        TranscriptionEvent::Progress {
                            job_id,
                            progress: Progress::Transcribing {
                                completed_units,
                                total_units,
                                message,
                            },
                        },
                    );
                };
                backend.transcribe(&request.input_path, request.language, &mut report)
            })
            .and_then(|output| {
                output.validate()?;
                Ok(output)
            });

        match result {
            Ok(output) => {
                emit(
                    events,
                    TranscriptionEvent::Succeeded {
                        job_id,
                        recording_id: request.recording_id,
                        attempts: attempt,
                        output,
                    },
                );
                return;
            }
            Err(error) if error.retryable && attempt < request.retry.max_attempts() => {
                let delay = request.retry.delay_after(attempt);
                release_loaded(events, loaded, ReleaseReason::Retry);
                emit(
                    events,
                    TranscriptionEvent::RetryScheduled {
                        job_id,
                        completed_attempt: attempt,
                        next_attempt: attempt + 1,
                        delay,
                        error,
                    },
                );
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
            }
            Err(error) => {
                emit(
                    events,
                    TranscriptionEvent::Failed {
                        job_id,
                        recording_id: request.recording_id,
                        attempts: attempt,
                        error,
                    },
                );
                return;
            }
        }
    }
}

fn ensure_backend<'a>(
    events: &mpsc::Sender<TranscriptionEvent>,
    job_id: JobId,
    selection: &ModelSelection,
    models: &mut dyn ModelProvider,
    factory: &mut dyn BackendFactory,
    loaded: &'a mut Option<LoadedBackend>,
) -> Result<&'a mut dyn TranscriptionBackend, TranscriptionError> {
    if loaded
        .as_ref()
        .is_some_and(|current| selection_accepts(selection, &current.model))
    {
        return Ok(&mut *loaded.as_mut().expect("checked above").backend);
    }

    let model = models.prepare(selection, &mut |progress| {
        emit(
            events,
            TranscriptionEvent::Progress {
                job_id,
                progress: Progress::PreparingModel(progress),
            },
        );
    })?;
    if loaded
        .as_ref()
        .is_some_and(|current| current.model.path != model.path)
    {
        release_loaded(events, loaded, ReleaseReason::ModelChanged);
    }
    if loaded.is_none() {
        let backend = factory.load(&model, &mut |progress| {
            emit(
                events,
                TranscriptionEvent::Progress {
                    job_id,
                    progress: Progress::LoadingBackend(progress),
                },
            );
        })?;
        *loaded = Some(LoadedBackend { model, backend });
    }
    Ok(&mut *loaded.as_mut().expect("loaded immediately above").backend)
}

fn selection_accepts(selection: &ModelSelection, model: &PreparedModel) -> bool {
    match selection {
        ModelSelection::Auto => true,
        ModelSelection::Kind(kind) => model.kind == *kind,
        ModelSelection::Path(path) => model.path == *path,
    }
}

fn release_loaded(
    events: &mpsc::Sender<TranscriptionEvent>,
    loaded: &mut Option<LoadedBackend>,
    reason: ReleaseReason,
) {
    if let Some(loaded) = loaded.take() {
        emit(
            events,
            TranscriptionEvent::ModelReleased {
                model: loaded.model,
                reason,
            },
        );
    }
}

fn emit(events: &mpsc::Sender<TranscriptionEvent>, event: TranscriptionEvent) {
    let _ = events.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelKind, ModelPreparationStage};
    use std::{
        collections::VecDeque,
        sync::{Condvar, Mutex},
        time::Instant,
    };

    #[derive(Clone)]
    struct FakeModels {
        prepares: Arc<AtomicU64>,
    }

    impl ModelProvider for FakeModels {
        fn prepare(
            &mut self,
            _selection: &ModelSelection,
            progress: &mut dyn FnMut(ModelPreparation),
        ) -> Result<PreparedModel, TranscriptionError> {
            self.prepares.fetch_add(1, Ordering::Relaxed);
            progress(ModelPreparation::new(
                ModelPreparationStage::Ready,
                100,
                Some(100),
                "model ready",
            ));
            Ok(PreparedModel::new(
                ModelKind::Compact,
                PathBuf::from("model.bin"),
            ))
        }
    }

    struct FakeFactory {
        loads: Arc<AtomicU64>,
        results: Arc<Mutex<VecDeque<Result<TranscriptionOutput, TranscriptionError>>>>,
        gate: Option<Arc<(Mutex<bool>, Condvar)>>,
    }

    impl BackendFactory for FakeFactory {
        fn load(
            &mut self,
            _model: &PreparedModel,
            progress: &mut dyn FnMut(LoadProgress),
        ) -> Result<Box<dyn TranscriptionBackend>, TranscriptionError> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            progress(LoadProgress::new(1, Some(1), "backend ready"));
            Ok(Box::new(FakeBackend {
                results: Arc::clone(&self.results),
                gate: self.gate.clone(),
            }))
        }
    }

    struct FakeBackend {
        results: Arc<Mutex<VecDeque<Result<TranscriptionOutput, TranscriptionError>>>>,
        gate: Option<Arc<(Mutex<bool>, Condvar)>>,
    }

    impl TranscriptionBackend for FakeBackend {
        fn transcribe(
            &mut self,
            _input_path: &Path,
            _language: Language,
            progress: &mut dyn FnMut(u64, Option<u64>, String),
        ) -> Result<TranscriptionOutput, TranscriptionError> {
            progress(50, Some(100), "halfway".to_owned());
            if let Some(gate) = &self.gate {
                let (lock, ready) = &**gate;
                let mut open = lock.lock().unwrap();
                while !*open {
                    open = ready.wait(open).unwrap();
                }
            }
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(output("fallback", 0.0, 1.0)))
        }
    }

    fn output(text: &str, start: f64, end: f64) -> TranscriptionOutput {
        TranscriptionOutput::new(
            text.to_owned(),
            vec![TranscriptSegment {
                start_seconds: start,
                end_seconds: end,
                text: text.to_owned(),
            }],
        )
    }

    fn request(id: &str) -> TranscriptionRequest {
        TranscriptionRequest::new(
            RecordingId::new(id).unwrap(),
            PathBuf::from(format!("{id}.mp4")),
        )
    }

    fn worker(
        config: WorkerConfig,
        results: Vec<Result<TranscriptionOutput, TranscriptionError>>,
        gate: Option<Arc<(Mutex<bool>, Condvar)>>,
    ) -> (TranscriptionWorker, Arc<AtomicU64>, Arc<AtomicU64>) {
        let prepares = Arc::new(AtomicU64::new(0));
        let loads = Arc::new(AtomicU64::new(0));
        let worker = TranscriptionWorker::spawn(
            config,
            FakeModels {
                prepares: Arc::clone(&prepares),
            },
            FakeFactory {
                loads: Arc::clone(&loads),
                results: Arc::new(Mutex::new(results.into())),
                gate,
            },
        )
        .unwrap();
        (worker, prepares, loads)
    }

    fn next_terminal(worker: &TranscriptionWorker) -> TranscriptionEvent {
        loop {
            let event = worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            if matches!(
                event,
                TranscriptionEvent::Succeeded { .. } | TranscriptionEvent::Failed { .. }
            ) {
                return event;
            }
        }
    }

    #[test]
    fn supported_language_codes_match_existing_settings() {
        for code in ["auto", "nl", "en", "fr", "de", "es"] {
            assert_eq!(Language::from_code(code).unwrap().code(), code);
        }
        assert_eq!(Language::from_code("xx"), None);
        assert_eq!(Language::Auto.whisper_language(), None);
        assert_eq!(Language::Dutch.whisper_language(), Some("nl"));
    }

    #[test]
    fn model_and_backend_are_loaded_lazily_then_reused() {
        let (worker, prepares, loads) = worker(
            WorkerConfig {
                idle_release: IdleReleasePolicy::Never,
                ..WorkerConfig::default()
            },
            vec![Ok(output("one", 0.0, 1.0)), Ok(output("two", 0.0, 1.0))],
            None,
        );
        assert_eq!(prepares.load(Ordering::Relaxed), 0);
        worker.try_submit(request("recording-one")).unwrap();
        assert!(matches!(
            next_terminal(&worker),
            TranscriptionEvent::Succeeded { .. }
        ));
        worker.try_submit(request("recording-two")).unwrap();
        assert!(matches!(
            next_terminal(&worker),
            TranscriptionEvent::Succeeded { .. }
        ));
        assert_eq!(prepares.load(Ordering::Relaxed), 1);
        assert_eq!(loads.load(Ordering::Relaxed), 1);
        worker.shutdown().unwrap();
    }

    #[test]
    fn queue_is_bounded_while_the_single_worker_is_busy() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (worker, _, _) = worker(
            WorkerConfig {
                queue_capacity: 1,
                idle_release: IdleReleasePolicy::Never,
            },
            vec![Ok(output("one", 0.0, 1.0)), Ok(output("two", 0.0, 1.0))],
            Some(Arc::clone(&gate)),
        );
        worker.try_submit(request("recording-one")).unwrap();
        loop {
            if matches!(
                worker
                    .events()
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                TranscriptionEvent::Started { .. }
            ) {
                break;
            }
        }
        worker.try_submit(request("recording-two")).unwrap();
        assert_eq!(
            worker.try_submit(request("recording-three")),
            Err(SubmitError::QueueFull)
        );
        let (lock, ready) = &*gate;
        *lock.lock().unwrap() = true;
        ready.notify_all();
        assert!(matches!(
            next_terminal(&worker),
            TranscriptionEvent::Succeeded { .. }
        ));
        assert!(matches!(
            next_terminal(&worker),
            TranscriptionEvent::Succeeded { .. }
        ));
        worker.shutdown().unwrap();
    }

    #[test]
    fn retryable_failure_releases_and_reloads_the_backend() {
        let failure = TranscriptionError::new(FailureKind::Inference, "temporary", true);
        let (worker, prepares, loads) = worker(
            WorkerConfig {
                idle_release: IdleReleasePolicy::Never,
                ..WorkerConfig::default()
            },
            vec![Err(failure.clone()), Ok(output("recovered", 0.0, 2.0))],
            None,
        );
        let mut request = request("recording-retry");
        request.retry = RetryPolicy::new(2, Duration::ZERO, Duration::ZERO).unwrap();
        worker.try_submit(request).unwrap();
        let mut saw_retry = false;
        loop {
            match worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
            {
                TranscriptionEvent::RetryScheduled { error, .. } => {
                    assert_eq!(error, failure);
                    saw_retry = true;
                }
                TranscriptionEvent::Succeeded { attempts, .. } => {
                    assert_eq!(attempts, 2);
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_retry);
        assert_eq!(prepares.load(Ordering::Relaxed), 2);
        assert_eq!(loads.load(Ordering::Relaxed), 2);
        worker.shutdown().unwrap();
    }

    #[test]
    fn invalid_timed_output_fails_without_retry() {
        let (worker, _, _) = worker(
            WorkerConfig::default(),
            vec![Ok(output("bad", 3.0, 2.0))],
            None,
        );
        worker.try_submit(request("recording-invalid")).unwrap();
        match next_terminal(&worker) {
            TranscriptionEvent::Failed {
                attempts, error, ..
            } => {
                assert_eq!(attempts, 1);
                assert_eq!(error.kind, FailureKind::InvalidOutput);
                assert!(!error.retryable);
            }
            event => panic!("unexpected event: {event:?}"),
        }
        worker.shutdown().unwrap();
    }

    #[test]
    fn after_each_job_policy_releases_the_model_explicitly() {
        let (worker, _, _) = worker(
            WorkerConfig {
                idle_release: IdleReleasePolicy::AfterEachJob,
                ..WorkerConfig::default()
            },
            vec![Ok(output("done", 0.0, 1.0))],
            None,
        );
        worker.try_submit(request("recording-release")).unwrap();
        let mut saw_success = false;
        loop {
            match worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
            {
                TranscriptionEvent::Succeeded { .. } => saw_success = true,
                TranscriptionEvent::ModelReleased { reason, .. } => {
                    assert!(saw_success);
                    assert_eq!(reason, ReleaseReason::AfterJob);
                    break;
                }
                _ => {}
            }
        }
        worker.shutdown().unwrap();
    }

    #[test]
    fn idle_timeout_releases_a_loaded_model() {
        let timeout = Duration::from_millis(20);
        let (worker, _, _) = worker(
            WorkerConfig {
                idle_release: IdleReleasePolicy::After(timeout),
                ..WorkerConfig::default()
            },
            vec![Ok(output("done", 0.0, 1.0))],
            None,
        );
        worker.try_submit(request("recording-idle")).unwrap();
        assert!(matches!(
            next_terminal(&worker),
            TranscriptionEvent::Succeeded { .. }
        ));
        let started = Instant::now();
        loop {
            if let TranscriptionEvent::ModelReleased { reason, .. } = worker
                .events()
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
            {
                assert_eq!(reason, ReleaseReason::IdleTimeout);
                assert!(started.elapsed() >= timeout);
                break;
            }
        }
        worker.shutdown().unwrap();
    }

    #[test]
    fn worker_rejects_invalid_capacity_and_idle_duration() {
        let models = FakeModels {
            prepares: Arc::new(AtomicU64::new(0)),
        };
        let factory = FakeFactory {
            loads: Arc::new(AtomicU64::new(0)),
            results: Arc::new(Mutex::new(VecDeque::new())),
            gate: None,
        };
        assert!(TranscriptionWorker::spawn(
            WorkerConfig {
                queue_capacity: 0,
                idle_release: IdleReleasePolicy::Never,
            },
            models.clone(),
            factory,
        )
        .is_err());

        let factory = FakeFactory {
            loads: Arc::new(AtomicU64::new(0)),
            results: Arc::new(Mutex::new(VecDeque::new())),
            gate: None,
        };
        assert!(TranscriptionWorker::spawn(
            WorkerConfig {
                queue_capacity: 1,
                idle_release: IdleReleasePolicy::After(Duration::ZERO),
            },
            models,
            factory,
        )
        .is_err());
    }
}
