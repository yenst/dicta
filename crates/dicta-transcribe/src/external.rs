use crate::{
    BackendFactory, FailureKind, Language, LoadProgress, ModelCatalog, ModelPreparation,
    ModelPreparationStage, ModelProvider, ModelSelection, PreparedModel, TranscriptionBackend,
    TranscriptionError, TranscriptionOutput,
};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

static TEMP_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessPlan {
    program: PathBuf,
    args: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
}

impl ProcessPlan {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            environment: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, value: impl Into<OsString>) -> Self {
        self.args.push(value.into());
        self
    }

    #[must_use]
    pub fn args(mut self, values: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        self.args.extend(values.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.args
    }

    #[must_use]
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.environment
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait ProcessExecutor: Send + Sync + 'static {
    fn executable_available(&self, program: &Path) -> bool;
    fn output(&self, plan: &ProcessPlan) -> io::Result<ProcessOutput>;

    fn output_with_file_progress(
        &self,
        plan: &ProcessPlan,
        tracked_file: &Path,
        progress: &mut dyn FnMut(u64) -> bool,
    ) -> io::Result<ProcessOutput> {
        let output = self.output(plan)?;
        let bytes = fs::metadata(tracked_file).map_or(0, |metadata| metadata.len());
        if progress(bytes) {
            Ok(output)
        } else {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "process operation was cancelled",
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcessExecutor;

impl ProcessExecutor for SystemProcessExecutor {
    fn executable_available(&self, program: &Path) -> bool {
        executable_available(program)
    }

    fn output(&self, plan: &ProcessPlan) -> io::Result<ProcessOutput> {
        let output = Command::new(plan.program())
            .args(plan.arguments())
            .envs(plan.environment().iter().cloned())
            .stdin(Stdio::null())
            .output()?;
        Ok(ProcessOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn output_with_file_progress(
        &self,
        plan: &ProcessPlan,
        tracked_file: &Path,
        progress: &mut dyn FnMut(u64) -> bool,
    ) -> io::Result<ProcessOutput> {
        let mut child = Command::new(plan.program())
            .args(plan.arguments())
            .envs(plan.environment().iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        loop {
            if child.try_wait()?.is_some() {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    pipe.read_to_end(&mut stdout)?;
                }
                if let Some(mut pipe) = child.stderr.take() {
                    pipe.read_to_end(&mut stderr)?;
                }
                let status = child.wait()?;
                return Ok(ProcessOutput {
                    success: status.success(),
                    code: status.code(),
                    stdout,
                    stderr,
                });
            }
            let bytes = fs::metadata(tracked_file).map_or(0, |metadata| metadata.len());
            if !progress(bytes) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "process operation was cancelled",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn executable_available(program: &Path) -> bool {
    if program.as_os_str().is_empty() {
        return false;
    }
    if program.is_absolute() || program.components().count() > 1 {
        return program.is_file();
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
    })
}

#[derive(Clone, Debug)]
pub struct ExistingModelProvider {
    catalog: ModelCatalog,
    rejected_paths: Vec<PathBuf>,
}

impl ExistingModelProvider {
    #[must_use]
    pub const fn new(catalog: ModelCatalog) -> Self {
        Self {
            catalog,
            rejected_paths: Vec::new(),
        }
    }

    #[must_use]
    pub fn rejecting_path(mut self, path: PathBuf) -> Self {
        self.rejected_paths.push(path);
        self
    }

    #[must_use]
    pub fn available_model(&self, selection: &ModelSelection) -> Option<PreparedModel> {
        self.catalog
            .candidates(selection)
            .into_iter()
            .find(|candidate| {
                !self
                    .rejected_paths
                    .iter()
                    .any(|path| path == &candidate.path)
                    && model_is_usable(&self.catalog, candidate)
            })
    }
}

impl ModelProvider for ExistingModelProvider {
    fn prepare(
        &mut self,
        selection: &ModelSelection,
        progress: &mut dyn FnMut(ModelPreparation),
    ) -> Result<PreparedModel, TranscriptionError> {
        progress(ModelPreparation::new(
            ModelPreparationStage::Locating,
            0,
            None,
            "locating a local Whisper model",
        ));
        let model = self.available_model(selection).ok_or_else(|| {
            TranscriptionError::new(
                FailureKind::ModelUnavailable,
                "no usable local Dicta Whisper model was found",
                false,
            )
        })?;
        let size = fs::metadata(&model.path).map_or(0, |metadata| metadata.len());
        progress(ModelPreparation::new(
            ModelPreparationStage::Ready,
            size,
            Some(size),
            format!("using {}", model.path.display()),
        ));
        Ok(model)
    }
}

fn model_is_usable(catalog: &ModelCatalog, model: &PreparedModel) -> bool {
    let Ok(metadata) = fs::symlink_metadata(&model.path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return false;
    }
    catalog
        .integrity(model.kind)
        .and_then(|integrity| integrity.minimum_size_bytes)
        .is_none_or(|minimum| metadata.len() >= minimum)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoxtypeBackendConfig {
    pub ffmpeg_program: PathBuf,
    pub voxtype_program: PathBuf,
    pub temporary_root: PathBuf,
}

impl Default for VoxtypeBackendConfig {
    fn default() -> Self {
        Self {
            ffmpeg_program: PathBuf::from("ffmpeg"),
            voxtype_program: PathBuf::from("voxtype"),
            temporary_root: std::env::temp_dir(),
        }
    }
}

pub struct VoxtypeBackendFactory {
    config: VoxtypeBackendConfig,
    executor: Arc<dyn ProcessExecutor>,
}

impl VoxtypeBackendFactory {
    #[must_use]
    pub fn system(config: VoxtypeBackendConfig) -> Self {
        Self::new(config, Arc::new(SystemProcessExecutor))
    }

    #[must_use]
    pub fn new(config: VoxtypeBackendConfig, executor: Arc<dyn ProcessExecutor>) -> Self {
        Self { config, executor }
    }

    #[must_use]
    pub fn is_available(&self) -> bool {
        self.executor
            .executable_available(&self.config.ffmpeg_program)
            && self
                .executor
                .executable_available(&self.config.voxtype_program)
    }
}

impl BackendFactory for VoxtypeBackendFactory {
    fn load(
        &mut self,
        model: &PreparedModel,
        progress: &mut dyn FnMut(LoadProgress),
    ) -> Result<Box<dyn TranscriptionBackend>, TranscriptionError> {
        if !self.is_available() {
            return Err(TranscriptionError::new(
                FailureKind::BackendLoad,
                "local transcription requires both ffmpeg and voxtype",
                false,
            ));
        }
        if !model.path.is_file() {
            return Err(TranscriptionError::new(
                FailureKind::ModelUnavailable,
                format!("Whisper model is unavailable: {}", model.path.display()),
                false,
            ));
        }
        progress(LoadProgress::new(
            1,
            Some(1),
            "Voxtype local Whisper backend ready",
        ));
        Ok(Box::new(VoxtypeBackend {
            config: self.config.clone(),
            executor: Arc::clone(&self.executor),
            model: model.clone(),
        }))
    }
}

struct VoxtypeBackend {
    config: VoxtypeBackendConfig,
    executor: Arc<dyn ProcessExecutor>,
    model: PreparedModel,
}

impl TranscriptionBackend for VoxtypeBackend {
    fn transcribe(
        &mut self,
        input_path: &Path,
        language: Language,
        progress: &mut dyn FnMut(u64, Option<u64>, String),
    ) -> Result<TranscriptionOutput, TranscriptionError> {
        let metadata = fs::symlink_metadata(input_path).map_err(|error| {
            TranscriptionError::new(
                FailureKind::Input,
                format!("could not inspect recording audio input: {error}"),
                false,
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(TranscriptionError::new(
                FailureKind::Input,
                "transcription input must be a regular file",
                false,
            ));
        }

        let temporary = TemporaryWorkspace::create(
            &self.config.temporary_root,
            &self.model.path,
            self.model.kind,
        )
        .map_err(|error| {
            TranscriptionError::new(
                FailureKind::Internal,
                format!("could not reserve temporary transcription workspace: {error}"),
                false,
            )
        })?;
        progress(0, Some(2), "extracting narration audio".to_owned());
        let extraction = self
            .executor
            .output(&ffmpeg_plan(
                &self.config.ffmpeg_program,
                input_path,
                temporary.audio_path(),
            ))
            .map_err(|error| process_io_error("ffmpeg", error))?;
        if !extraction.success {
            return Err(process_failure("ffmpeg", &extraction, FailureKind::Input));
        }

        progress(1, Some(2), "transcribing narration locally".to_owned());
        let inference = self
            .executor
            .output(&voxtype_plan(
                &self.config.voxtype_program,
                temporary.data_root(),
                self.model.kind,
                temporary.audio_path(),
                language,
            ))
            .map_err(|error| process_io_error("voxtype", error))?;
        if !inference.success {
            return Err(process_failure(
                "voxtype",
                &inference,
                FailureKind::Inference,
            ));
        }
        let transcript = parse_voxtype_stdout(&inference.stdout);
        if transcript.is_empty() {
            return Err(TranscriptionError::new(
                FailureKind::InvalidOutput,
                "Voxtype returned no detected speech",
                false,
            ));
        }
        progress(2, Some(2), "transcription complete".to_owned());
        let mut output = TranscriptionOutput::new(transcript, Vec::new());
        output.detected_language = language.whisper_language().map(str::to_owned);
        Ok(output)
    }
}

fn parse_voxtype_stdout(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("Loading audio file:")
                && !line.starts_with("Audio format:")
                && !line.starts_with("Processing ")
        })
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

fn ffmpeg_plan(program: &Path, input: &Path, output: &Path) -> ProcessPlan {
    ProcessPlan::new(program).args([
        OsString::from("-nostdin"),
        OsString::from("-hide_banner"),
        OsString::from("-loglevel"),
        OsString::from("error"),
        OsString::from("-y"),
        OsString::from("-i"),
        input.as_os_str().to_owned(),
        OsString::from("-vn"),
        OsString::from("-ac"),
        OsString::from("1"),
        OsString::from("-ar"),
        OsString::from("16000"),
        OsString::from("-c:a"),
        OsString::from("pcm_s16le"),
        output.as_os_str().to_owned(),
    ])
}

fn voxtype_plan(
    program: &Path,
    data_root: &Path,
    model: crate::ModelKind,
    input: &Path,
    language: Language,
) -> ProcessPlan {
    ProcessPlan::new(program)
        .env("XDG_DATA_HOME", data_root.as_os_str())
        .args([
            OsString::from("--quiet"),
            OsString::from("--engine"),
            OsString::from("whisper"),
            OsString::from("--whisper-mode"),
            OsString::from("local"),
            OsString::from("--model"),
            OsString::from(voxtype_model_name(model)),
            OsString::from("--language"),
            OsString::from(language.code()),
            OsString::from("--initial-prompt"),
            OsString::from(technical_prompt(language)),
            OsString::from("transcribe"),
            input.as_os_str().to_owned(),
        ])
}

const fn voxtype_model_name(model: crate::ModelKind) -> &'static str {
    match model {
        crate::ModelKind::Compact => "base",
        crate::ModelKind::LargeV3Turbo => "large-v3-turbo",
    }
}

const fn voxtype_model_filename(model: crate::ModelKind) -> &'static str {
    match model {
        crate::ModelKind::Compact => "ggml-base.bin",
        crate::ModelKind::LargeV3Turbo => "ggml-large-v3-turbo.bin",
    }
}

const fn technical_prompt(language: Language) -> &'static str {
    if matches!(language, Language::Dutch) {
        "Nederlandse technische uitleg over softwareontwikkeling, API-integraties, broncode en implementatiedetails."
    } else {
        "Technical software explanation about APIs, source code, and implementation details."
    }
}

fn process_io_error(program: &str, error: io::Error) -> TranscriptionError {
    TranscriptionError::new(
        FailureKind::BackendLoad,
        format!("could not start {program}: {error}"),
        false,
    )
}

fn process_failure(program: &str, output: &ProcessOutput, kind: FailureKind) -> TranscriptionError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    let message = if detail.is_empty() {
        format!("{program} failed with status {:?}", output.code)
    } else {
        format!("{program} failed: {detail}")
    };
    TranscriptionError::new(kind, message, false)
}

struct TemporaryWorkspace {
    directory: PathBuf,
    audio: PathBuf,
    data_root: PathBuf,
    models_directory: PathBuf,
    model_alias: PathBuf,
}

impl TemporaryWorkspace {
    fn create(root: &Path, model: &Path, model_kind: crate::ModelKind) -> io::Result<Self> {
        for _ in 0..32 {
            let sequence = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let directory = root.join(format!(
                "dicta-transcription-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    let audio = directory.join("narration.wav");
                    let data_root = directory.join("data");
                    let models_directory = data_root.join("voxtype/models");
                    if let Err(error) = fs::create_dir_all(&models_directory) {
                        cleanup_workspace(&directory, &data_root, &models_directory, None);
                        return Err(error);
                    }
                    let model_alias = models_directory.join(voxtype_model_filename(model_kind));
                    if let Err(error) = create_model_alias(model, &model_alias) {
                        cleanup_workspace(
                            &directory,
                            &data_root,
                            &models_directory,
                            Some(&model_alias),
                        );
                        return Err(error);
                    }
                    return Ok(Self {
                        directory,
                        audio,
                        data_root,
                        models_directory,
                        model_alias,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a unique transcription directory",
        ))
    }

    fn audio_path(&self) -> &Path {
        &self.audio
    }

    fn data_root(&self) -> &Path {
        &self.data_root
    }
}

#[cfg(unix)]
fn create_model_alias(model: &Path, alias: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(model, alias)
}

#[cfg(not(unix))]
fn create_model_alias(_model: &Path, _alias: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Voxtype model staging requires a Unix filesystem",
    ))
}

fn cleanup_workspace(
    directory: &Path,
    data_root: &Path,
    models_directory: &Path,
    model_alias: Option<&Path>,
) {
    if let Some(model_alias) = model_alias {
        let _ = fs::remove_file(model_alias);
    }
    let _ = fs::remove_dir(models_directory);
    let _ = fs::remove_dir(data_root.join("voxtype"));
    let _ = fs::remove_dir(data_root);
    let _ = fs::remove_dir(directory);
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.audio);
        cleanup_workspace(
            &self.directory,
            &self.data_root,
            &self.models_directory,
            Some(&self.model_alias),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelKind, TranscriptionRequest, TranscriptionWorker, WorkerConfig};
    use dicta_core::RecordingId;
    use std::{collections::VecDeque, sync::Mutex};

    #[derive(Default)]
    struct FakeExecutor {
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
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing fake output"))
        }
    }

    fn success(stdout: &[u8]) -> ProcessOutput {
        ProcessOutput {
            success: true,
            code: Some(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    fn fixture_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dicta-transcribe-{label}-{}-{}",
            std::process::id(),
            TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn existing_provider_uses_the_first_safe_model() {
        let root = fixture_root("model");
        fs::create_dir_all(&root).unwrap();
        let compact = root.join(crate::COMPACT_FILENAME);
        fs::write(&compact, b"model").unwrap();
        let provider = ExistingModelProvider::new(ModelCatalog::new(root.clone()));
        assert_eq!(
            provider.available_model(&ModelSelection::Auto),
            Some(PreparedModel::new(ModelKind::Compact, compact.clone()))
        );
        fs::remove_file(compact).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn worker_runs_shell_free_ffmpeg_and_voxtype_plans() {
        let root = fixture_root("worker");
        let models = root.join("models");
        let temporary = root.join("temporary");
        fs::create_dir_all(&models).unwrap();
        fs::create_dir_all(&temporary).unwrap();
        let model = models.join(crate::COMPACT_FILENAME);
        let input = root.join("recording.mp4");
        fs::write(&model, b"model").unwrap();
        fs::write(&input, b"recording").unwrap();
        let executor = Arc::new(FakeExecutor {
            plans: Mutex::new(Vec::new()),
            outputs: Mutex::new(VecDeque::from([
                success(&[]),
                success(b"  fixed   the overflow\n"),
            ])),
            available: true,
        });
        let provider = ExistingModelProvider::new(ModelCatalog::new(models.clone()));
        let factory = VoxtypeBackendFactory::new(
            VoxtypeBackendConfig {
                ffmpeg_program: PathBuf::from("ffmpeg"),
                voxtype_program: PathBuf::from("voxtype"),
                temporary_root: temporary.clone(),
            },
            executor.clone(),
        );
        let worker =
            TranscriptionWorker::spawn(WorkerConfig::default(), provider, factory).unwrap();
        let recording_id = RecordingId::new("recording-1").unwrap();
        worker
            .try_submit(TranscriptionRequest::new(
                recording_id.clone(),
                input.clone(),
            ))
            .unwrap();
        let output = loop {
            match worker.events().recv().unwrap() {
                crate::TranscriptionEvent::Succeeded { output, .. } => break output,
                crate::TranscriptionEvent::Failed { error, .. } => panic!("{error}"),
                _ => {}
            }
        };
        assert_eq!(output.transcript, "fixed the overflow");
        worker.shutdown().unwrap();

        let plans = executor.plans.lock().unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].program(), Path::new("ffmpeg"));
        assert!(plans[0].arguments().contains(&OsString::from("-nostdin")));
        assert_eq!(plans[1].program(), Path::new("voxtype"));
        assert!(!plans[1].arguments().contains(&model.into_os_string()));
        assert!(plans[1].arguments().contains(&OsString::from("base")));
        assert!(plans[1].arguments().contains(&OsString::from("local")));
        assert_eq!(plans[1].environment().len(), 1);
        assert_eq!(plans[1].environment()[0].0, OsString::from("XDG_DATA_HOME"));
        drop(plans);
        assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);

        fs::remove_file(input).unwrap();
        fs::remove_file(models.join(crate::COMPACT_FILENAME)).unwrap();
        fs::remove_dir(models).unwrap();
        fs::remove_dir(temporary).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn unavailable_tools_fail_before_inference() {
        let root = fixture_root("unavailable");
        fs::create_dir_all(&root).unwrap();
        let model = root.join(crate::COMPACT_FILENAME);
        fs::write(&model, b"model").unwrap();
        let executor = Arc::new(FakeExecutor::default());
        let mut factory = VoxtypeBackendFactory::new(VoxtypeBackendConfig::default(), executor);
        let error = match factory.load(
            &PreparedModel::new(ModelKind::Compact, model.clone()),
            &mut |_| {},
        ) {
            Ok(_) => panic!("unavailable tools must reject backend loading"),
            Err(error) => error,
        };
        assert_eq!(error.kind, FailureKind::BackendLoad);
        fs::remove_file(model).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn voxtype_status_lines_are_not_persisted_as_transcript_text() {
        let stdout = br#"Loading audio file: \"/tmp/narration.wav\"
Audio format: 16000 Hz, 1 channel(s), Int
Processing 76224 samples (4.76s)...
This is the spoken result.
Second sentence.
"#;
        assert_eq!(
            parse_voxtype_stdout(stdout),
            "This is the spoken result. Second sentence."
        );
    }
}
