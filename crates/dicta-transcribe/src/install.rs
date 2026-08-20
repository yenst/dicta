use crate::{
    ExistingModelProvider, ModelCatalog, ModelKind, ModelPreparation, ModelPreparationStage,
    ModelSelection, PreparedModel, ProcessExecutor, ProcessOutput, ProcessPlan,
    LARGE_V3_TURBO_DOWNLOAD_BYTES,
};
use std::{
    error::Error,
    ffi::OsString,
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelFileState {
    Missing,
    Partial,
    Ready,
    Invalid,
    Unverified,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedModelStatus {
    pub kind: ModelKind,
    pub path: PathBuf,
    pub state: ModelFileState,
    pub size_bytes: u64,
    pub expected_download_bytes: u64,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelStatus {
    pub active_model: Option<PreparedModel>,
    pub quality: ManagedModelStatus,
    pub install_progress: Option<ModelPreparation>,
    pub install_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelInstallDisposition {
    Installed,
    AlreadyPresent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelInstallOutcome {
    pub disposition: ModelInstallDisposition,
    pub status: ModelStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelInstallFailure {
    Cancelled,
    ToolUnavailable,
    UnsafePath,
    Download,
    Integrity,
    Io,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelInstallError {
    pub kind: ModelInstallFailure,
    pub message: String,
}

impl ModelInstallError {
    fn new(kind: ModelInstallFailure, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ModelInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ModelInstallError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelInstallerConfig {
    pub curl_program: PathBuf,
    pub sha1sum_program: PathBuf,
}

impl Default for ModelInstallerConfig {
    fn default() -> Self {
        Self {
            curl_program: PathBuf::from("curl"),
            sha1sum_program: PathBuf::from("sha1sum"),
        }
    }
}

#[derive(Clone)]
pub struct ModelInstaller {
    catalog: ModelCatalog,
    config: ModelInstallerConfig,
    executor: Arc<dyn ProcessExecutor>,
    verified: Arc<Mutex<Option<VerifiedModelIdentity>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VerifiedModelIdentity {
    path: PathBuf,
    size_bytes: u64,
    modified: Option<SystemTime>,
}

impl ModelInstaller {
    #[must_use]
    pub fn new(
        catalog: ModelCatalog,
        config: ModelInstallerConfig,
        executor: Arc<dyn ProcessExecutor>,
    ) -> Self {
        Self {
            catalog,
            config,
            executor,
            verified: Arc::new(Mutex::new(None)),
        }
    }

    #[must_use]
    pub fn system(catalog: ModelCatalog, config: ModelInstallerConfig) -> Self {
        Self::new(catalog, config, Arc::new(crate::SystemProcessExecutor))
    }

    #[must_use]
    pub fn status(&self) -> ModelStatus {
        let quality = self.quality_status();
        let active_model = self
            .trusted_provider()
            .available_model(&ModelSelection::Auto);
        ModelStatus {
            active_model,
            quality,
            install_progress: None,
            install_error: None,
        }
    }

    /// Returns a provider that never selects the managed quality model until
    /// this manager has verified its current path/size/mtime identity.
    #[must_use]
    pub fn trusted_provider(&self) -> ExistingModelProvider {
        let target = self.catalog.managed_path(ModelKind::LargeV3Turbo);
        let provider = ExistingModelProvider::new(self.catalog.clone());
        if self.cached_identity_matches(&target) {
            provider
        } else {
            provider.rejecting_path(target)
        }
    }

    /// Installs the managed high-quality model through structured system
    /// processes. Returning `false` from `progress` cancels the active curl
    /// child and removes the same-directory partial file.
    ///
    /// # Errors
    /// Returns a typed tool, path, download, integrity, cancellation, or I/O
    /// failure. A failed installation never replaces the existing target.
    pub fn install_quality(
        &self,
        progress: &mut dyn FnMut(ModelPreparation) -> bool,
    ) -> Result<ModelInstallOutcome, ModelInstallError> {
        let initial = self.status();
        if initial.quality.state == ModelFileState::Ready {
            return Ok(ModelInstallOutcome {
                disposition: ModelInstallDisposition::AlreadyPresent,
                status: initial,
            });
        }
        if !self
            .executor
            .executable_available(&self.config.curl_program)
        {
            return Err(ModelInstallError::new(
                ModelInstallFailure::ToolUnavailable,
                "quality model installation requires curl",
            ));
        }
        if !self
            .executor
            .executable_available(&self.config.sha1sum_program)
        {
            return Err(ModelInstallError::new(
                ModelInstallFailure::ToolUnavailable,
                "quality model installation requires sha1sum",
            ));
        }

        let kind = ModelKind::LargeV3Turbo;
        let target = self.catalog.managed_path(kind);
        let parent = target.parent().ok_or_else(|| {
            ModelInstallError::new(
                ModelInstallFailure::UnsafePath,
                "quality model target has no parent directory",
            )
        })?;
        prepare_parent(parent)?;
        reject_symlink(&target, "quality model target")?;
        let partial = partial_path(&target)?;
        reject_symlink(&partial, "quality model partial")?;
        remove_stale_partial(&partial)?;
        let mut guard = PartialGuard::new(partial.clone());

        let url = self.catalog.download_url(kind).ok_or_else(|| {
            ModelInstallError::new(
                ModelInstallFailure::Download,
                "quality model has no download URL",
            )
        })?;
        let download = curl_plan(&self.config.curl_program, &partial, url);
        let total = LARGE_V3_TURBO_DOWNLOAD_BYTES;
        if !progress(ModelPreparation::new(
            ModelPreparationStage::Downloading,
            0,
            Some(total),
            "downloading the high-quality model",
        )) {
            return Err(cancelled());
        }
        let output = self
            .executor
            .output_with_file_progress(&download, &partial, &mut |downloaded| {
                progress(ModelPreparation::new(
                    ModelPreparationStage::Downloading,
                    downloaded,
                    Some(total),
                    "downloading the high-quality model",
                ))
            })
            .map_err(|error| {
                if error.kind() == io::ErrorKind::Interrupted {
                    cancelled()
                } else {
                    ModelInstallError::new(
                        ModelInstallFailure::Download,
                        format!("could not run curl: {error}"),
                    )
                }
            })?;
        if !output.success {
            return Err(process_error(
                "curl",
                &output,
                ModelInstallFailure::Download,
            ));
        }

        let metadata = regular_file_metadata(&partial, "downloaded quality model")?;
        let integrity = self.catalog.integrity(kind).ok_or_else(|| {
            ModelInstallError::new(
                ModelInstallFailure::Integrity,
                "quality model has no integrity metadata",
            )
        })?;
        if integrity
            .minimum_size_bytes
            .is_some_and(|minimum| metadata.len() < minimum)
        {
            return Err(ModelInstallError::new(
                ModelInstallFailure::Integrity,
                format!(
                    "downloaded quality model is too small: {} bytes",
                    metadata.len()
                ),
            ));
        }
        if !progress(ModelPreparation::new(
            ModelPreparationStage::Verifying,
            metadata.len(),
            Some(metadata.len()),
            "verifying the high-quality model",
        )) {
            return Err(cancelled());
        }
        verify_sha1(
            self.executor.as_ref(),
            &self.config.sha1sum_program,
            &partial,
            &integrity.digest,
        )?;
        fs::File::open(&partial)
            .and_then(|file| file.sync_all())
            .map_err(|error| io_error("sync verified quality model", error))?;
        fs::rename(&partial, &target)
            .map_err(|error| io_error("atomically install verified quality model", error))?;
        guard.disarm();
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        let size_bytes = fs::metadata(&target).map_or(metadata.len(), |value| value.len());
        self.cache_verified_identity(&target);
        let _ = progress(ModelPreparation::new(
            ModelPreparationStage::Ready,
            size_bytes,
            Some(size_bytes),
            "high-quality transcription is ready",
        ));
        Ok(ModelInstallOutcome {
            disposition: ModelInstallDisposition::Installed,
            status: ModelStatus {
                active_model: self
                    .trusted_provider()
                    .available_model(&ModelSelection::Auto),
                quality: ManagedModelStatus {
                    kind,
                    path: target,
                    state: ModelFileState::Ready,
                    size_bytes,
                    expected_download_bytes: total,
                    detail: "the managed high-quality model passed SHA-1 verification".to_owned(),
                },
                install_progress: None,
                install_error: None,
            },
        })
    }

    fn quality_status(&self) -> ManagedModelStatus {
        let kind = ModelKind::LargeV3Turbo;
        let path = self.catalog.managed_path(kind);
        let partial = partial_path(&path).ok();
        let expected_download_bytes = LARGE_V3_TURBO_DOWNLOAD_BYTES;
        let base = |state, size_bytes, detail| ManagedModelStatus {
            kind,
            path: path.clone(),
            state,
            size_bytes,
            expected_download_bytes,
            detail,
        };
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return base(
                    ModelFileState::Invalid,
                    0,
                    "the managed model path is a symbolic link".to_owned(),
                );
            }
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                return base(
                    ModelFileState::Invalid,
                    0,
                    "the managed model path is not a regular file".to_owned(),
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some((partial, metadata)) = partial.and_then(|partial| {
                    fs::symlink_metadata(&partial)
                        .ok()
                        .filter(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                        .map(|metadata| (partial, metadata))
                }) {
                    return base(
                        ModelFileState::Partial,
                        metadata.len(),
                        format!("an incomplete download exists at {}", partial.display()),
                    );
                }
                return base(
                    ModelFileState::Missing,
                    0,
                    "the managed high-quality model is not installed".to_owned(),
                );
            }
            Err(error) => {
                return base(
                    ModelFileState::Invalid,
                    0,
                    format!("could not inspect the managed model: {error}"),
                );
            }
        };
        let Some(integrity) = self.catalog.integrity(kind) else {
            return base(
                ModelFileState::Unverified,
                metadata.len(),
                "the managed model has no integrity metadata".to_owned(),
            );
        };
        if integrity
            .minimum_size_bytes
            .is_some_and(|minimum| metadata.len() < minimum)
        {
            return base(
                ModelFileState::Invalid,
                metadata.len(),
                "the managed model is smaller than the trusted minimum".to_owned(),
            );
        }
        if self.cached_identity_matches_metadata(&path, &metadata) {
            return base(
                ModelFileState::Ready,
                metadata.len(),
                "the cached model identity previously passed SHA-1 verification".to_owned(),
            );
        }
        if !self
            .executor
            .executable_available(&self.config.sha1sum_program)
        {
            return base(
                ModelFileState::Unverified,
                metadata.len(),
                "sha1sum is unavailable; model integrity was not checked".to_owned(),
            );
        }
        match verify_sha1(
            self.executor.as_ref(),
            &self.config.sha1sum_program,
            &path,
            &integrity.digest,
        ) {
            Ok(()) => {
                self.cache_verified_identity(&path);
                base(
                    ModelFileState::Ready,
                    metadata.len(),
                    "the managed high-quality model passed SHA-1 verification".to_owned(),
                )
            }
            Err(error) => {
                self.clear_verified_identity();
                base(ModelFileState::Invalid, metadata.len(), error.message)
            }
        }
    }

    fn cached_identity_matches(&self, path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && self.cached_identity_matches_metadata(path, &metadata)
        })
    }

    fn cached_identity_matches_metadata(&self, path: &Path, metadata: &fs::Metadata) -> bool {
        let cached = self
            .verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cached.as_ref().is_some_and(|identity| {
            identity.path == path
                && identity.size_bytes == metadata.len()
                && identity.modified == metadata.modified().ok()
        })
    }

    fn cache_verified_identity(&self, path: &Path) {
        let identity = fs::symlink_metadata(path)
            .ok()
            .map(|metadata| VerifiedModelIdentity {
                path: path.to_path_buf(),
                size_bytes: metadata.len(),
                modified: metadata.modified().ok(),
            });
        *self
            .verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = identity;
    }

    fn clear_verified_identity(&self) {
        *self
            .verified
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}

fn curl_plan(program: &Path, partial: &Path, url: &str) -> ProcessPlan {
    ProcessPlan::new(program).args([
        OsString::from("--location"),
        OsString::from("--fail"),
        OsString::from("--silent"),
        OsString::from("--show-error"),
        OsString::from("--proto"),
        OsString::from("=https"),
        OsString::from("--connect-timeout"),
        OsString::from("30"),
        OsString::from("--output"),
        partial.as_os_str().to_owned(),
        OsString::from(url),
    ])
}

fn verify_sha1(
    executor: &dyn ProcessExecutor,
    program: &Path,
    path: &Path,
    expected: &str,
) -> Result<(), ModelInstallError> {
    let output = executor
        .output(&ProcessPlan::new(program).arg(path.as_os_str()))
        .map_err(|error| io_error("run sha1sum", error))?;
    if !output.success {
        return Err(process_error(
            "sha1sum",
            &output,
            ModelInstallFailure::Integrity,
        ));
    }
    let actual = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if expected.len() != 40
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        || actual != expected.to_ascii_lowercase()
    {
        return Err(ModelInstallError::new(
            ModelInstallFailure::Integrity,
            "the quality model did not pass SHA-1 integrity verification",
        ));
    }
    Ok(())
}

fn prepare_parent(parent: &Path) -> Result<(), ModelInstallError> {
    fs::create_dir_all(parent).map_err(|error| io_error("create model directory", error))?;
    let metadata =
        fs::symlink_metadata(parent).map_err(|error| io_error("inspect model directory", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ModelInstallError::new(
            ModelInstallFailure::UnsafePath,
            "model directory must be a real directory, not a symbolic link",
        ));
    }
    Ok(())
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), ModelInstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ModelInstallError::new(
            ModelInstallFailure::UnsafePath,
            format!("{label} must not be a symbolic link"),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(&format!("inspect {label}"), error)),
    }
}

fn remove_stale_partial(path: &Path) -> Result<(), ModelInstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(path).map_err(|error| io_error("remove stale model partial", error))
        }
        Ok(_) => Err(ModelInstallError::new(
            ModelInstallFailure::UnsafePath,
            "model partial must be a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("inspect model partial", error)),
    }
}

fn regular_file_metadata(path: &Path, label: &str) -> Result<fs::Metadata, ModelInstallError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(label, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ModelInstallError::new(
            ModelInstallFailure::UnsafePath,
            format!("{label} is not a regular file"),
        ));
    }
    Ok(metadata)
}

fn partial_path(target: &Path) -> Result<PathBuf, ModelInstallError> {
    let filename = target.file_name().ok_or_else(|| {
        ModelInstallError::new(
            ModelInstallFailure::UnsafePath,
            "quality model target has no filename",
        )
    })?;
    let mut partial = OsString::from(".");
    partial.push(filename);
    partial.push(".partial");
    Ok(target.with_file_name(partial))
}

fn process_error(
    program: &str,
    output: &ProcessOutput,
    kind: ModelInstallFailure,
) -> ModelInstallError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    ModelInstallError::new(
        kind,
        if detail.is_empty() {
            format!("{program} failed with status {:?}", output.code)
        } else {
            format!("{program} failed: {detail}")
        },
    )
}

fn cancelled() -> ModelInstallError {
    ModelInstallError::new(
        ModelInstallFailure::Cancelled,
        "quality model installation was cancelled",
    )
}

fn io_error(action: &str, error: io::Error) -> ModelInstallError {
    ModelInstallError::new(
        ModelInstallFailure::Io,
        format!("could not {action}: {error}"),
    )
}

struct PartialGuard {
    path: PathBuf,
    armed: bool,
}

impl PartialGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessOutput, LARGE_V3_TURBO_FILENAME, LARGE_V3_TURBO_SHA1};
    use std::{
        collections::VecDeque,
        sync::{atomic::AtomicU64, Mutex},
    };

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(1);

    struct FakeExecutor {
        checksum: String,
        plans: Mutex<Vec<ProcessPlan>>,
        outputs: Mutex<VecDeque<ProcessOutput>>,
        available: bool,
    }

    impl ProcessExecutor for FakeExecutor {
        fn executable_available(&self, _program: &Path) -> bool {
            self.available
        }

        fn output(&self, plan: &ProcessPlan) -> io::Result<ProcessOutput> {
            self.plans.lock().unwrap().push(plan.clone());
            if plan.program() == Path::new("sha1sum") {
                return Ok(ProcessOutput {
                    success: true,
                    code: Some(0),
                    stdout: format!("{}  model.bin\n", self.checksum).into_bytes(),
                    stderr: Vec::new(),
                });
            }
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing output"))
        }

        fn output_with_file_progress(
            &self,
            plan: &ProcessPlan,
            tracked_file: &Path,
            progress: &mut dyn FnMut(u64) -> bool,
        ) -> io::Result<ProcessOutput> {
            self.plans.lock().unwrap().push(plan.clone());
            let file = fs::File::create(tracked_file)?;
            file.set_len(501 * 1024 * 1024)?;
            if !progress(501 * 1024 * 1024) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            Ok(ProcessOutput {
                success: true,
                code: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    fn fixture_root(label: &str) -> PathBuf {
        use std::sync::atomic::Ordering;
        std::env::temp_dir().join(format!(
            "dicta-model-install-{label}-{}-{}",
            std::process::id(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn manager(root: &Path, checksum: &str) -> (ModelInstaller, Arc<FakeExecutor>) {
        let executor = Arc::new(FakeExecutor {
            checksum: checksum.to_owned(),
            plans: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::new()),
            available: true,
        });
        let manager = ModelInstaller::new(
            ModelCatalog::new(root.join("models")),
            ModelInstallerConfig::default(),
            executor.clone(),
        );
        (manager, executor)
    }

    #[test]
    fn status_distinguishes_missing_and_partial_quality_models() {
        let root = fixture_root("status");
        let (manager, _) = manager(&root, LARGE_V3_TURBO_SHA1);
        assert_eq!(manager.status().quality.state, ModelFileState::Missing);
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(
            root.join("models")
                .join(format!(".{LARGE_V3_TURBO_FILENAME}.partial")),
            b"partial",
        )
        .unwrap();
        let status = manager.status().quality;
        assert_eq!(status.state, ModelFileState::Partial);
        assert_eq!(status.size_bytes, 7);
        fs::remove_file(
            root.join("models")
                .join(format!(".{LARGE_V3_TURBO_FILENAME}.partial")),
        )
        .unwrap();
        fs::remove_dir(root.join("models")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn install_verifies_and_atomically_promotes_a_same_directory_partial() {
        let root = fixture_root("success");
        let (manager, executor) = manager(&root, LARGE_V3_TURBO_SHA1);
        let mut stages = Vec::new();
        let outcome = manager
            .install_quality(&mut |progress| {
                stages.push(progress.stage);
                true
            })
            .unwrap();
        assert_eq!(outcome.disposition, ModelInstallDisposition::Installed);
        assert_eq!(outcome.status.quality.state, ModelFileState::Ready);
        let target = root.join("models").join(LARGE_V3_TURBO_FILENAME);
        assert!(target.is_file());
        assert!(!partial_path(&target).unwrap().exists());
        assert!(stages.contains(&ModelPreparationStage::Downloading));
        assert!(stages.contains(&ModelPreparationStage::Verifying));
        assert!(stages.contains(&ModelPreparationStage::Ready));
        let plans = executor.plans.lock().unwrap();
        assert_eq!(plans[0].program(), Path::new("curl"));
        assert!(plans[0].arguments().contains(&OsString::from("=https")));
        assert_eq!(plans[1].program(), Path::new("sha1sum"));
        drop(plans);
        fs::remove_file(target).unwrap();
        fs::remove_dir(root.join("models")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn integrity_failure_removes_partial_without_replacing_existing_target() {
        let root = fixture_root("integrity");
        fs::create_dir_all(root.join("models")).unwrap();
        let target = root.join("models").join(LARGE_V3_TURBO_FILENAME);
        fs::write(&target, b"existing invalid model").unwrap();
        let (manager, _) = manager(&root, "0000000000000000000000000000000000000000");
        let error = manager.install_quality(&mut |_| true).unwrap_err();
        assert_eq!(error.kind, ModelInstallFailure::Integrity);
        assert_eq!(fs::read(&target).unwrap(), b"existing invalid model");
        assert!(!partial_path(&target).unwrap().exists());
        fs::remove_file(target).unwrap();
        fs::remove_dir(root.join("models")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn cancellation_removes_partial_and_never_creates_target() {
        let root = fixture_root("cancel");
        let (manager, _) = manager(&root, LARGE_V3_TURBO_SHA1);
        let error = manager
            .install_quality(&mut |progress| {
                progress.stage != ModelPreparationStage::Downloading
                    || progress.completed_bytes == 0
            })
            .unwrap_err();
        assert_eq!(error.kind, ModelInstallFailure::Cancelled);
        let target = root.join("models").join(LARGE_V3_TURBO_FILENAME);
        assert!(!target.exists());
        assert!(!partial_path(&target).unwrap().exists());
        fs::remove_dir(root.join("models")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn known_invalid_managed_quality_never_becomes_the_active_model() {
        let root = fixture_root("invalid-active");
        let models = root.join("models");
        fs::create_dir_all(&models).unwrap();
        let target = models.join(LARGE_V3_TURBO_FILENAME);
        fs::File::create(&target)
            .unwrap()
            .set_len(501 * 1024 * 1024)
            .unwrap();
        let compact = root.join(crate::COMPACT_FILENAME);
        fs::write(&compact, b"compact").unwrap();
        let executor = Arc::new(FakeExecutor {
            checksum: "0000000000000000000000000000000000000000".to_owned(),
            plans: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::new()),
            available: true,
        });
        let manager = ModelInstaller::new(
            ModelCatalog::new(models.clone()).with_bundled_compact(compact.clone()),
            ModelInstallerConfig::default(),
            executor,
        );
        assert_eq!(
            manager
                .trusted_provider()
                .available_model(&ModelSelection::Auto)
                .unwrap()
                .path,
            compact
        );
        let status = manager.status();
        assert_eq!(status.quality.state, ModelFileState::Invalid);
        assert_eq!(status.active_model.unwrap().path, compact);
        fs::remove_file(target).unwrap();
        fs::remove_file(compact).unwrap();
        fs::remove_dir(models).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
