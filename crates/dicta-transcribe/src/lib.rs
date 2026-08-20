//! Lean, UI-independent local transcription orchestration for Dicta.
//!
//! This crate deliberately does not depend on Whisper, an async runtime, or a UI
//! toolkit. Applications inject model preparation and inference implementations;
//! [`TranscriptionWorker`] supplies the bounded single-worker queue, lifecycle,
//! retries, and typed events around them.

mod external;
mod install;
mod model;
mod retry;
mod worker;

pub use external::{
    ExistingModelProvider, ProcessExecutor, ProcessOutput, ProcessPlan, SystemProcessExecutor,
    VoxtypeBackendConfig, VoxtypeBackendFactory,
};
pub use install::{
    ManagedModelStatus, ModelFileState, ModelInstallDisposition, ModelInstallError,
    ModelInstallFailure, ModelInstallOutcome, ModelInstaller, ModelInstallerConfig, ModelStatus,
};
pub use model::{
    IntegrityAlgorithm, IntegritySpec, ModelCatalog, ModelKind, ModelPreparation,
    ModelPreparationStage, ModelSelection, PreparedModel, COMPACT_FILENAME,
    LARGE_V3_TURBO_DOWNLOAD_BYTES, LARGE_V3_TURBO_FILENAME, LARGE_V3_TURBO_LEGACY_FILENAME,
    LARGE_V3_TURBO_SHA1, LARGE_V3_TURBO_URL,
};
pub use retry::retry_candidates;
pub use worker::{
    BackendFactory, FailureKind, IdleReleasePolicy, JobId, Language, LoadProgress,
    ModelIntegrityVerifier, ModelProvider, Progress, ReleaseReason, RetryPolicy, SubmitError,
    TranscriptionBackend, TranscriptionError, TranscriptionEvent, TranscriptionOutput,
    TranscriptionQueue, TranscriptionRequest, TranscriptionWorker, WorkerConfig,
};
