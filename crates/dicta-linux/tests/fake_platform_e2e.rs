use dicta_capture::{
    AudioSource, CaptureCapabilities, CaptureChild, CaptureOutput, CommandOutput, CommandPlan,
    Geometry, OutputTransform, Platform, ProcessExit, SessionKind, ToolCapabilities,
};
use dicta_control::{AnnotationTool, Command, RequestEnvelope, RequestId, ResponsePayload};
use dicta_core::{
    storage::{annotation_sidecar_path, read_json, write_json_atomic, AppSettings},
    AnnotationCanvas, AnnotationFile, ProjectFile, ProjectId, RecordingFile, RecordingId,
    RecordingScope, TimelineNote, TranscriptSegment, TranscriptionStatus,
};
use dicta_engine::RecordingSession;
use dicta_linux::{
    CaptureStartObserver, DisabledTranscriptionPort, FilesystemIdSource, LinuxCapture, LinuxConfig,
    LinuxStorage, StorageLayout,
};
use dicta_runtime::{
    AnnotationPort, CapturePort, Clock, PortError, PortErrorKind, Runtime, RuntimeConfig,
    StoragePort,
};
use dicta_transcribe::TranscriptionOutput;
use std::{
    cell::RefCell,
    ffi::OsStr,
    fs, io,
    num::NonZeroU64,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct ChildState {
    wait_calls: usize,
    killed: bool,
    exit_after_start: bool,
}

struct FakeChild {
    state: Arc<Mutex<ChildState>>,
}

impl CaptureChild for FakeChild {
    fn id(&self) -> u32 {
        4242
    }

    fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
        let mut state = self.state.lock().unwrap();
        state.wait_calls += 1;
        Ok(
            (state.exit_after_start && state.wait_calls > 1).then_some(ProcessExit {
                success: true,
                code: Some(0),
            }),
        )
    }

    fn kill(&mut self) -> io::Result<()> {
        self.state.lock().unwrap().killed = true;
        Ok(())
    }

    fn wait(&mut self) -> io::Result<ProcessExit> {
        Ok(ProcessExit {
            success: false,
            code: None,
        })
    }
}

struct FakePlatform {
    child: Option<Box<dyn CaptureChild>>,
    spawned: Rc<RefCell<Vec<CommandPlan>>>,
}

impl Platform for FakePlatform {
    fn executable_exists(&self, _name: &OsStr) -> bool {
        true
    }

    fn output(&mut self, _plan: &CommandPlan) -> io::Result<CommandOutput> {
        Ok(CommandOutput::success(Vec::new()))
    }

    fn spawn(&mut self, plan: &CommandPlan) -> io::Result<Box<dyn CaptureChild>> {
        self.spawned.borrow_mut().push(plan.clone());
        let arguments = plan.arguments();
        let output_index = arguments
            .iter()
            .position(|argument| argument == "-o" || argument == "--file")
            .ok_or_else(|| io::Error::other("capture plan omitted output"))?;
        let output = arguments
            .get(output_index + 1)
            .ok_or_else(|| io::Error::other("capture plan omitted output path"))?;
        fs::write(PathBuf::from(output), b"fake video")?;
        self.child
            .take()
            .ok_or_else(|| io::Error::other("fake recorder already spawned"))
    }

    fn sleep(&mut self, _duration: Duration) {}
}

#[derive(Clone, Copy)]
struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH
    }
}

#[derive(Default)]
struct FakeAnnotations {
    recording_id: Option<RecordingId>,
}

impl AnnotationPort for FakeAnnotations {
    fn set_enabled(&mut self, recording_id: &RecordingId, _enabled: bool) -> Result<(), PortError> {
        self.recording_id = Some(recording_id.clone());
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

    fn finish(&mut self, recording_id: &RecordingId) -> Result<Option<AnnotationFile>, PortError> {
        Ok(Some(AnnotationFile::new(
            recording_id.clone(),
            AnnotationCanvas {
                output_name: Some("DP-1".to_owned()),
                width_pixels: 1920,
                height_pixels: 1080,
                scale: 1.0,
                extra: serde_json::Map::new(),
            },
        )))
    }
}

struct StartObserver(Rc<RefCell<Vec<(RecordingId, String)>>>);

impl CaptureStartObserver for StartObserver {
    fn recording_started(
        &mut self,
        session: &RecordingSession,
        output: &CaptureOutput,
    ) -> Result<(), PortError> {
        self.0
            .borrow_mut()
            .push((session.recording_id.clone(), output.name.clone()));
        Ok(())
    }
}

type FakeLinuxRuntime = Runtime<
    LinuxCapture<FakePlatform, StartObserver>,
    DisabledTranscriptionPort,
    FakeAnnotations,
    LinuxStorage<FixedClock>,
    FixedClock,
    FilesystemIdSource,
>;

fn fake_linux_runtime(root: &Path) -> FakeLinuxRuntime {
    let child_state = Arc::new(Mutex::new(ChildState::default()));
    child_state.lock().unwrap().exit_after_start = true;
    let platform = FakePlatform {
        child: Some(Box::new(FakeChild { state: child_state })),
        spawned: Rc::new(RefCell::new(Vec::new())),
    };
    let config = LinuxConfig::new(root, "DP-1");
    let capture = LinuxCapture::from_capabilities(
        platform,
        capabilities(),
        config,
        StartObserver(Rc::new(RefCell::new(Vec::new()))),
    )
    .unwrap();
    let layout = StorageLayout::new(root);
    Runtime::new(
        capture,
        DisabledTranscriptionPort::default(),
        FakeAnnotations::default(),
        LinuxStorage::new(layout.clone(), FixedClock),
        FixedClock,
        FilesystemIdSource::new(layout),
        RuntimeConfig {
            transcribe_after_recording: false,
        },
    )
}

fn capabilities() -> CaptureCapabilities {
    CaptureCapabilities {
        session: SessionKind::HyprlandWayland,
        tools: ToolCapabilities {
            gpu_screen_recorder: true,
            wf_recorder: true,
            hyprctl: true,
            pactl: false,
            pw_dump: false,
            kill: true,
        },
        outputs: vec![CaptureOutput {
            name: "DP-1".to_owned(),
            description: "Main".to_owned(),
            geometry: Geometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale: 1.0,
            pixel_size: (1920, 1080),
            transform: OutputTransform::Normal,
            refresh_hz: 60.0,
            focused: true,
        }],
        audio_sources: Vec::<AudioSource>::new(),
    }
}

fn request(id: u64, command: Command) -> RequestEnvelope {
    RequestEnvelope::new(RequestId::new(NonZeroU64::new(id).unwrap()), command)
}

#[test]
fn fake_platform_records_observes_and_atomically_persists_core_models() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-e2e-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    let layout = StorageLayout::new(&root);
    let spawned = Rc::new(RefCell::new(Vec::new()));
    let child_state = Arc::new(Mutex::new(ChildState::default()));
    child_state.lock().unwrap().exit_after_start = true;
    let platform = FakePlatform {
        child: Some(Box::new(FakeChild {
            state: Arc::clone(&child_state),
        })),
        spawned: Rc::clone(&spawned),
    };
    let starts = Rc::new(RefCell::new(Vec::new()));
    let config = LinuxConfig::new(&root, "DP-1");
    let capture = LinuxCapture::from_capabilities(
        platform,
        capabilities(),
        config,
        StartObserver(Rc::clone(&starts)),
    )
    .unwrap();
    let mut runtime = Runtime::new(
        capture,
        DisabledTranscriptionPort::default(),
        FakeAnnotations::default(),
        LinuxStorage::new(layout.clone(), FixedClock),
        FixedClock,
        FilesystemIdSource::new(layout.clone()),
        RuntimeConfig {
            transcribe_after_recording: false,
        },
    );

    let started = runtime.handle(request(
        1,
        Command::RecordStart {
            project: None,
            note: Some("explain the native path".to_owned()),
        },
    ));
    assert!(matches!(
        started.response.payload,
        ResponsePayload::Success { .. }
    ));
    assert_eq!(starts.borrow().len(), 1);
    let recording_id = starts.borrow()[0].0.clone();
    assert_eq!(starts.borrow()[0].1, "DP-1");
    assert!(matches!(
        runtime
            .handle(request(2, Command::RecordStop))
            .response
            .payload,
        ResponsePayload::Success { .. }
    ));

    let video_path = layout.video_path(None, &recording_id);
    let metadata_path = layout.metadata_path(None, &recording_id);
    let annotation_path = annotation_sidecar_path(&metadata_path);
    assert_eq!(fs::read(&video_path).unwrap(), b"fake video");
    let metadata: RecordingFile = read_json(&metadata_path).unwrap();
    assert_eq!(metadata.id, recording_id);
    assert_eq!(metadata.note, "explain the native path");
    assert_eq!(metadata.size_bytes, Some(10));
    assert_eq!(metadata.extra["capture_backend"], "gpu-screen-recorder");
    assert_eq!(
        metadata.annotation_path.as_deref(),
        Some(annotation_path.to_string_lossy().as_ref())
    );
    let annotations: AnnotationFile = read_json(&annotation_path).unwrap();
    assert_eq!(annotations.recording_id, metadata.id);
    assert_eq!(spawned.borrow().len(), 1);
    assert_eq!(spawned.borrow()[0].program(), "gpu-screen-recorder");
    assert!(!child_state.lock().unwrap().killed);
    assert!(fs::read_dir(metadata_path.parent().unwrap())
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));

    drop(runtime);
    fs::remove_dir_all(root).unwrap();
}

struct FailingObserver;

impl CaptureStartObserver for FailingObserver {
    fn recording_started(
        &mut self,
        _session: &RecordingSession,
        _output: &CaptureOutput,
    ) -> Result<(), PortError> {
        Err(PortError::new(
            PortErrorKind::Unavailable,
            "overlay failed to open",
        ))
    }
}

#[test]
fn observer_failure_aborts_the_just_started_recorder_and_removes_staging() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-observer-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    let child_state = Arc::new(Mutex::new(ChildState::default()));
    let platform = FakePlatform {
        child: Some(Box::new(FakeChild {
            state: Arc::clone(&child_state),
        })),
        spawned: Rc::new(RefCell::new(Vec::new())),
    };
    let config = LinuxConfig::new(&root, "DP-1");
    let mut capture =
        LinuxCapture::from_capabilities(platform, capabilities(), config, FailingObserver).unwrap();
    let session = RecordingSession {
        recording_id: RecordingId::new("20260820-18-00-00").unwrap(),
        project_id: None,
        note: None,
    };

    let error = capture.start(&session).unwrap_err();
    assert_eq!(error.kind, PortErrorKind::Unavailable);
    assert!(!capture.is_recording());
    assert!(child_state.lock().unwrap().killed);
    let video_path = StorageLayout::new(&root).video_path(None, &session.recording_id);
    assert!(!video_path.exists());
    assert!(fs::read_dir(video_path.parent().unwrap())
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".part")));

    fs::remove_dir_all(root).unwrap();
}

fn catalog_recording(root: &std::path::Path, id: &str) -> RecordingFile {
    let recording_id = RecordingId::new(id).unwrap();
    let project_id = ProjectId::new("demo").unwrap();
    let layout = StorageLayout::new(root);
    RecordingFile {
        id: recording_id.clone(),
        project_id: project_id.clone(),
        video_path: layout
            .video_path(Some(&project_id), &recording_id)
            .to_string_lossy()
            .into_owned(),
        metadata_path: layout
            .metadata_path(Some(&project_id), &recording_id)
            .to_string_lossy()
            .into_owned(),
        note: "catalog fixture".to_owned(),
        recording_scope: RecordingScope::Repository,
        git_branch: Some("main".to_owned()),
        started_at: None,
        ended_at: None,
        duration_seconds: Some(4.0),
        size_bytes: Some(5),
        success: true,
        transcript: None,
        transcript_path: None,
        transcript_segments: Vec::new(),
        transcription_status: TranscriptionStatus::Complete,
        transcription_error: None,
        transcription_language: Some("en".to_owned()),
        poster_path: None,
        annotation_path: None,
        timeline_notes: Vec::new(),
        extra: serde_json::Map::new(),
    }
}

fn write_catalog_fixture(root: &std::path::Path, id: &str) -> RecordingFile {
    let project_dir = root.join("demo");
    let project = ProjectFile {
        id: ProjectId::new("demo").unwrap(),
        name: "Demo".to_owned(),
        created_at: std::time::UNIX_EPOCH.into(),
        source_path: None,
        extra: serde_json::Map::new(),
    };
    write_json_atomic(&project_dir.join("project.json"), &project).unwrap();
    let recording = catalog_recording(root, id);
    let metadata = PathBuf::from(&recording.metadata_path);
    write_json_atomic(&metadata, &recording).unwrap();
    fs::write(PathBuf::from(&recording.video_path), b"video").unwrap();
    let stem = metadata.file_stem().unwrap().to_string_lossy();
    fs::write(
        metadata.with_file_name(format!("{stem}.transcript.md")),
        "catalog transcript",
    )
    .unwrap();
    recording
}

#[test]
fn production_storage_catalog_reuses_registered_project_and_recording_layout() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-catalog-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    let recording = write_catalog_fixture(&root, "20260820-19-00-00");
    let metadata = PathBuf::from(&recording.metadata_path);
    let unrelated = metadata.parent().unwrap().join("keep.txt");
    fs::write(&unrelated, "keep").unwrap();
    let mut storage = LinuxStorage::new(StorageLayout::new(&root), FixedClock);

    let projects = storage.load_projects().unwrap();
    assert_eq!(projects.len(), 2);
    assert!(projects.iter().any(|project| project.id.as_str() == "demo"));
    assert!(projects
        .iter()
        .any(|project| project.id.as_str() == dicta_core::GENERAL_PROJECT_ID));
    let recordings = storage.load_recordings().unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].id, recording.id);
    assert_eq!(
        recordings[0].transcript.as_deref(),
        Some("catalog transcript")
    );
    assert!(Path::new(&recordings[0].metadata_path).is_absolute());

    storage.delete_recording(&recordings[0]).unwrap();
    assert!(!metadata.exists());
    assert!(!Path::new(&recording.video_path).exists());
    assert!(unrelated.exists());
    assert!(storage.load_recordings().unwrap().is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn production_storage_persists_timeline_notes_at_the_catalog_identity() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-timeline-notes-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    let recording = write_catalog_fixture(&root, "20260820-19-00-10");
    let metadata = PathBuf::from(&recording.metadata_path);
    let mut storage = LinuxStorage::new(StorageLayout::new(&root), FixedClock);
    let catalog = storage.load_recordings().unwrap();
    let notes = vec![TimelineNote {
        id: "note-1".to_owned(),
        timestamp_seconds: 2.5,
        text: "Check the annotation toolbar".to_owned(),
        created_at: SystemTime::UNIX_EPOCH.into(),
        source: "typed".to_owned(),
        extra: serde_json::Map::new(),
    }];

    let updated = storage.save_timeline_notes(&catalog[0], &notes).unwrap();
    assert_eq!(updated.timeline_notes, notes);
    let persisted: RecordingFile = read_json(&metadata).unwrap();
    assert_eq!(persisted.timeline_notes, notes);
    assert!(fs::read_dir(metadata.parent().unwrap())
        .unwrap()
        .all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn production_storage_discovers_restart_retries_without_blocking_and_persists_state() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-retry-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut pending = write_catalog_fixture(&root, "20260820-19-00-01");
    pending.transcription_status = TranscriptionStatus::Pending;
    pending.transcription_error = Some("interrupted".to_owned());
    write_json_atomic(Path::new(&pending.metadata_path), &pending).unwrap();
    let mut failed = write_catalog_fixture(&root, "20260820-19-00-02");
    failed.transcription_status = TranscriptionStatus::Failed;
    failed.transcription_error = Some("worker stopped".to_owned());
    write_json_atomic(Path::new(&failed.metadata_path), &failed).unwrap();

    let started = Instant::now();
    let mut storage = LinuxStorage::system(StorageLayout::new(&root)).with_retry_discovery();
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "constructor waited for the catalog scan"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let first = loop {
        if let Some(candidate) = storage.poll_transcription_retry() {
            break candidate.unwrap();
        }
        assert!(Instant::now() < deadline, "retry scan did not complete");
        std::thread::yield_now();
    };
    let second = storage.poll_transcription_retry().unwrap().unwrap();
    assert_eq!(first.id, pending.id, "pending retries must run first");
    assert_eq!(second.id, failed.id);
    assert!(storage.poll_transcription_retry().is_none());

    storage.mark_transcription_pending(&failed.id).unwrap();
    let persisted: RecordingFile = read_json(Path::new(&failed.metadata_path)).unwrap();
    assert_eq!(persisted.transcription_status, TranscriptionStatus::Pending);
    assert!(persisted.transcription_error.is_none());
    storage
        .mark_transcription_failed(&failed.id, "  inference crashed  ")
        .unwrap();
    let persisted: RecordingFile = read_json(Path::new(&failed.metadata_path)).unwrap();
    assert_eq!(persisted.transcription_status, TranscriptionStatus::Failed);
    assert_eq!(
        persisted.transcription_error.as_deref(),
        Some("inference crashed")
    );
    assert!(
        fs::read_dir(Path::new(&failed.metadata_path).parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn production_storage_delete_rejects_symlinked_artifacts_before_mutation() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "dicta-linux-delete-security-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    let recording = write_catalog_fixture(&root, "20260820-20-00-00");
    let video = PathBuf::from(&recording.video_path);
    fs::remove_file(&video).unwrap();
    let outside = root.join("outside-video");
    fs::write(&outside, "outside").unwrap();
    symlink(&outside, &video).unwrap();
    let mut storage = LinuxStorage::new(StorageLayout::new(&root), FixedClock);
    let catalog = storage.load_recordings().unwrap();

    let error = storage.delete_recording(&catalog[0]).unwrap_err();
    assert_eq!(error.kind, PortErrorKind::PermissionDenied);
    assert!(Path::new(&recording.metadata_path).exists());
    assert_eq!(fs::read_to_string(outside).unwrap(), "outside");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn linked_branch_catalog_drives_transcription_persistence_at_the_resolved_path() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-linked-catalog-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let repository = root.join("repository");
    let registration = root.join("storage/demo/project.json");
    let project = ProjectFile {
        id: ProjectId::new("demo").unwrap(),
        name: "Demo".to_owned(),
        created_at: std::time::UNIX_EPOCH.into(),
        source_path: Some(repository.to_string_lossy().into_owned()),
        extra: serde_json::Map::new(),
    };
    write_json_atomic(&registration, &project).unwrap();
    let recording_id = RecordingId::new("20260820-21-00-00").unwrap();
    let metadata = repository
        .join(".dicta/branches/main/recordings/2026-08-20")
        .join(format!("{recording_id}.json"));
    let mut recording = catalog_recording(&root.join("storage"), recording_id.as_str());
    recording.metadata_path = metadata.to_string_lossy().into_owned();
    recording.video_path = metadata
        .with_extension("mp4")
        .to_string_lossy()
        .into_owned();
    write_json_atomic(&metadata, &recording).unwrap();
    fs::write(&recording.video_path, "video").unwrap();
    let mut storage = LinuxStorage::new(StorageLayout::new(root.join("storage")), FixedClock);

    let loaded = storage.load_recordings().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        loaded[0].metadata_path,
        metadata.canonicalize().unwrap().to_string_lossy()
    );
    storage
        .save_transcription(
            &recording_id,
            &TranscriptionOutput {
                transcript: "linked branch transcript".to_owned(),
                segments: vec![TranscriptSegment {
                    start_seconds: 0.0,
                    end_seconds: 1.0,
                    text: "linked branch transcript".to_owned(),
                }],
                detected_language: Some("en".to_owned()),
            },
        )
        .unwrap();
    let persisted: RecordingFile = read_json(&metadata).unwrap();
    assert_eq!(
        persisted.transcript.as_deref(),
        Some("linked branch transcript")
    );
    assert_eq!(
        persisted.transcription_status,
        TranscriptionStatus::Complete
    );

    let mut failed = persisted;
    failed.transcription_status = TranscriptionStatus::Failed;
    failed.transcription_error = Some("interrupted".to_owned());
    write_json_atomic(&metadata, &failed).unwrap();
    let mut restarted =
        LinuxStorage::system(StorageLayout::new(root.join("storage"))).with_retry_discovery();
    let deadline = Instant::now() + Duration::from_secs(2);
    let retry = loop {
        if let Some(candidate) = restarted.poll_transcription_retry() {
            break candidate.unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "linked retry scan did not complete"
        );
        std::thread::yield_now();
    };
    assert_eq!(
        retry.metadata_path,
        metadata.canonicalize().unwrap().to_string_lossy()
    );
    restarted.mark_transcription_pending(&retry.id).unwrap();
    let persisted: RecordingFile = read_json(&metadata).unwrap();
    assert_eq!(persisted.transcription_status, TranscriptionStatus::Pending);
    assert!(persisted.transcription_error.is_none());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_link_create_and_remove_preserve_repository_local_storage() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-project-crud-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let storage_root = root.join("storage");
    let repository = root.join("repository");
    fs::create_dir_all(&storage_root).unwrap();
    assert!(ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .status()
        .unwrap()
        .success());
    fs::create_dir_all(repository.join("nested")).unwrap();
    let mut storage = LinuxStorage::new(StorageLayout::new(&storage_root), FixedClock);

    let linked = storage
        .add_project(
            repository.join("nested").to_string_lossy().as_ref(),
            Some("Linked Demo"),
        )
        .unwrap();
    assert_eq!(linked.name, "Linked Demo");
    assert_eq!(
        Path::new(linked.source_path.as_deref().unwrap()),
        repository.canonicalize().unwrap()
    );
    assert!(repository.join(".dicta/project.json").is_file());
    assert!(repository
        .join(".dicta/branches/v2-main/recordings")
        .is_dir());
    assert!(fs::read_to_string(repository.join(".git/info/exclude"))
        .unwrap()
        .lines()
        .any(|line| line == ".dicta/"));
    let repeated = storage
        .add_project(repository.to_string_lossy().as_ref(), None)
        .unwrap();
    assert_eq!(repeated.id, linked.id);

    let first_created = storage.create_project("Scratch").unwrap();
    let second_created = storage.create_project("Scratch").unwrap();
    assert_ne!(first_created.id, second_created.id);
    assert!(storage_root
        .join(first_created.id.as_str())
        .join("recordings")
        .is_dir());

    storage.remove_project(&linked.id).unwrap();
    let registration = storage_root.join(linked.id.as_str());
    assert!(!registration.join("project.json").exists());
    assert!(fs::read_dir(&registration).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("project.removed-")));
    assert!(repository.join(".dicta/project.json").is_file());
    let general = ProjectId::new(dicta_core::GENERAL_PROJECT_ID).unwrap();
    assert_eq!(
        storage.remove_project(&general).unwrap_err().kind,
        PortErrorKind::PermissionDenied
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn linked_git_worktree_is_independent_confined_and_recordable() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-real-worktree-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let storage_root = root.join("storage");
    let repository = root.join("repository");
    let worktree = root.join("linked");
    fs::create_dir_all(&storage_root).unwrap();
    assert!(ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .status()
        .unwrap()
        .success());
    git(
        &repository,
        &["config", "user.email", "dicta@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Dicta Test"]);
    fs::write(repository.join("tracked"), "fixture").unwrap();
    git(&repository, &["add", "tracked"]);
    git(&repository, &["commit", "-m", "fixture"]);
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            "feature/worktree",
            worktree.to_str().unwrap(),
        ],
    );

    let mut storage = LinuxStorage::new(StorageLayout::new(&storage_root), FixedClock);
    let linked = storage
        .add_project(worktree.to_string_lossy().as_ref(), Some("Linked worktree"))
        .unwrap();
    let main = storage
        .add_project(repository.to_string_lossy().as_ref(), Some("Main checkout"))
        .unwrap();
    assert_ne!(linked.id, main.id);
    assert_eq!(
        Path::new(linked.source_path.as_deref().unwrap()),
        worktree.canonicalize().unwrap()
    );
    assert!(fs::read_to_string(repository.join(".git/info/exclude"))
        .unwrap()
        .lines()
        .any(|line| line == ".dicta/"));

    let mut runtime = fake_linux_runtime(&storage_root);
    runtime.handle(request(
        70,
        Command::ProjectSelect {
            project: linked.id.to_string(),
        },
    ));
    runtime.handle(request(
        71,
        Command::RecordStart {
            project: None,
            note: Some("linked worktree recording".to_owned()),
        },
    ));
    runtime.handle(request(72, Command::RecordStop));
    drop(runtime);

    let recordings = storage.load_recordings().unwrap();
    let recorded = recordings
        .iter()
        .find(|recording| recording.project_id == linked.id)
        .unwrap();
    assert_eq!(recorded.git_branch.as_deref(), Some("feature/worktree"));
    assert!(Path::new(&recorded.video_path).starts_with(worktree.join(".dicta")));
    storage.remove_project(&linked.id).unwrap();
    assert!(worktree.join(".dicta/project.json").is_file());

    let malicious = root.join("malicious");
    let outside = root.join("outside-admin");
    fs::create_dir_all(&malicious).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("sentinel"), "untouched").unwrap();
    fs::write(
        malicious.join(".git"),
        format!("gitdir: {}\n", outside.display()),
    )
    .unwrap();
    assert!(storage
        .add_project(malicious.to_string_lossy().as_ref(), None)
        .is_err());
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).unwrap(),
        "untouched"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn linked_live_capture_honors_branch_locking_for_paths_and_metadata() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-branch-capture-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let storage_root = root.join("storage");
    let repository = root.join("repository");
    fs::create_dir_all(&storage_root).unwrap();
    assert!(ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .status()
        .unwrap()
        .success());
    let linked = LinuxStorage::new(StorageLayout::new(&storage_root), FixedClock)
        .add_project(repository.to_string_lossy().as_ref(), Some("Linked"))
        .unwrap();

    let mut branch_runtime = fake_linux_runtime(&storage_root);
    assert!(matches!(
        branch_runtime
            .handle(request(
                80,
                Command::ProjectSelect {
                    project: linked.id.to_string(),
                },
            ))
            .response
            .payload,
        ResponsePayload::Success { .. }
    ));
    branch_runtime.handle(request(
        81,
        Command::RecordStart {
            project: None,
            note: Some("branch scoped".to_owned()),
        },
    ));
    branch_runtime.handle(request(82, Command::RecordStop));
    drop(branch_runtime);

    let mut repository_runtime = fake_linux_runtime(&storage_root);
    repository_runtime.handle(request(
        83,
        Command::SettingsSetBranchLocking { enabled: false },
    ));
    repository_runtime.handle(request(
        84,
        Command::ProjectSelect {
            project: linked.id.to_string(),
        },
    ));
    repository_runtime.handle(request(
        85,
        Command::RecordStart {
            project: None,
            note: Some("repository scoped".to_owned()),
        },
    ));
    repository_runtime.handle(request(86, Command::RecordStop));
    drop(repository_runtime);

    let settings: AppSettings = read_json(&storage_root.join("settings.json")).unwrap();
    assert!(!settings.branch_locking);
    let mut catalog = LinuxStorage::new(StorageLayout::new(&storage_root), FixedClock);
    let recordings = catalog.load_recordings().unwrap();
    assert_eq!(recordings.len(), 2);
    let branch = recordings
        .iter()
        .find(|recording| recording.note == "branch scoped")
        .unwrap();
    assert_eq!(branch.recording_scope, RecordingScope::Branch);
    assert_eq!(branch.git_branch.as_deref(), Some("main"));
    assert!(Path::new(&branch.video_path)
        .starts_with(repository.join(".dicta/branches/v2-main/recordings")));
    let repository_recording = recordings
        .iter()
        .find(|recording| recording.note == "repository scoped")
        .unwrap();
    assert_eq!(
        repository_recording.recording_scope,
        RecordingScope::Repository
    );
    assert_eq!(repository_recording.git_branch, None);
    assert!(Path::new(&repository_recording.video_path)
        .starts_with(repository.join(".dicta/recordings")));

    fs::remove_dir_all(root).unwrap();
}

fn git(repository: &Path, arguments: &[&str]) {
    assert!(ProcessCommand::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .status()
        .unwrap()
        .success());
}

fn commit_file(repository: &Path, branch: &str, file: &str) -> String {
    git(repository, &["checkout", "-b", branch]);
    fs::write(repository.join(file), branch).unwrap();
    git(repository, &["add", file]);
    git(repository, &["commit", "-m", branch]);
    dicta_core::git::output(repository, &["rev-parse", "HEAD"]).unwrap()
}

fn cleanup_packet(repository: &Path, branch: &str, oid: String) -> (PathBuf, PathBuf) {
    let path = repository
        .join(".dicta/branches")
        .join(dicta_core::branch::folder_name(branch));
    fs::create_dir_all(path.join("recordings/2026-08-20")).unwrap();
    write_json_atomic(
        &path.join("branch.json"),
        &dicta_core::BranchMetadata {
            git_branch: branch.to_owned(),
            head_oid: Some(oid),
        },
    )
    .unwrap();
    let video = path.join("recordings/2026-08-20/fixture.mp4");
    let metadata = path.join("recordings/2026-08-20/fixture.json");
    fs::write(&video, b"video-data").unwrap();
    fs::write(&metadata, b"metadata stays").unwrap();
    (video, metadata)
}

#[test]
fn merged_video_cleanup_deletes_only_proven_merged_branch_videos() {
    let root = std::env::temp_dir().join(format!(
        "dicta-linux-cleanup-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let storage_root = root.join("storage");
    let repository = root.join("repository");
    fs::create_dir_all(&storage_root).unwrap();
    assert!(ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .status()
        .unwrap()
        .success());
    git(
        &repository,
        &["config", "user.email", "dicta@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Dicta Test"]);
    fs::write(repository.join("base.txt"), "base").unwrap();
    git(&repository, &["add", "base.txt"]);
    git(&repository, &["commit", "-m", "base"]);
    let mut storage = LinuxStorage::new(StorageLayout::new(&storage_root), FixedClock);
    let linked = storage
        .add_project(repository.to_string_lossy().as_ref(), Some("Cleanup"))
        .unwrap();

    let done_oid = commit_file(&repository, "feature/done", "done.txt");
    git(&repository, &["checkout", "main"]);
    git(
        &repository,
        &["merge", "--no-ff", "feature/done", "-m", "merge done"],
    );
    let wip_oid = commit_file(&repository, "feature/wip", "wip.txt");
    git(&repository, &["checkout", "main"]);

    let (done_video, done_metadata) = cleanup_packet(&repository, "feature/done", done_oid);
    let (wip_video, _wip_metadata) = cleanup_packet(&repository, "feature/wip", wip_oid);

    let summary = storage.cleanup_merged_videos(&linked.id).unwrap();
    assert_eq!(summary.removed_files, 1);
    assert_eq!(summary.freed_bytes, 10);
    assert_eq!(summary.cleaned_branches, ["feature/done"]);
    assert_eq!(summary.default_branch.as_deref(), Some("main"));
    assert!(!done_video.exists());
    assert!(done_metadata.exists());
    assert!(wip_video.exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn project_link_rejects_symlinked_repository_storage() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!(
        "dicta-linux-project-symlink-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let storage_root = root.join("storage");
    let repository = root.join("repository");
    fs::create_dir_all(&storage_root).unwrap();
    assert!(ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .arg(&repository)
        .status()
        .unwrap()
        .success());
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, repository.join(".dicta")).unwrap();
    let mut storage = LinuxStorage::new(StorageLayout::new(&storage_root), FixedClock);

    let error = storage
        .add_project(repository.to_string_lossy().as_ref(), None)
        .unwrap_err();
    assert_eq!(error.kind, PortErrorKind::PermissionDenied);
    assert!(fs::read_dir(&storage_root).unwrap().next().is_none());

    fs::remove_dir_all(root).unwrap();
}
