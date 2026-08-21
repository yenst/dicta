use crate::{
    config::day_from_recording_id, omarchy::OmarchyShortcutIntegration, poster, SettingsStore,
    StorageLayout, SystemClock,
};
use chrono::{DateTime, Utc};
use dicta_capture::{CaptureArtifact, CaptureBackend};
use dicta_core::{
    catalog::{self, CatalogSource},
    storage::{self, annotation_sidecar_path, read_json, write_json_atomic},
    AnnotationFile, ProjectId, RecordingFile, RecordingId, RecordingScope, TimelineNote,
    TranscriptionStatus,
};
use dicta_engine::RecordingSession;
use dicta_runtime::{Clock, PortError, PortErrorKind, StoragePort};
use dicta_transcribe::TranscriptionOutput;
use serde_json::{json, Map};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, OpenOptions},
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

struct RecordingLocation {
    metadata_path: PathBuf,
    scope: RecordingScope,
    git_branch: Option<String>,
}

pub(crate) fn prepare_capture_path(
    layout: &StorageLayout,
    session: &RecordingSession,
) -> Result<PathBuf, PortError> {
    let tree = active_recording_tree(layout, session.project_id.as_ref())?;
    ensure_real_directory(&tree, "recording storage")?;
    let recordings = tree.join("recordings");
    ensure_real_directory(&recordings, "recordings folder")?;
    let day = recordings.join(day_from_recording_id(session.recording_id.as_str()));
    ensure_real_directory(&day, "recording day")?;
    Ok(day.join(format!("{}.mp4", session.recording_id)))
}

fn active_recording_tree(
    layout: &StorageLayout,
    project_id: Option<&ProjectId>,
) -> Result<PathBuf, PortError> {
    let settings = SettingsStore::new(layout.root()).load()?;
    let Some(project_id) = project_id.filter(|id| id.as_str() != dicta_core::GENERAL_PROJECT_ID)
    else {
        return Ok(storage::general_storage_path(
            layout.root(),
            settings.general_path.as_deref(),
        ));
    };
    let registration = project_registration(layout, project_id)?;
    let project: dicta_core::ProjectFile = read_json(&registration.join("project.json"))
        .map_err(|error| storage_port_error("read project registration", &error))?;
    if project.id != *project_id {
        return Err(PortError::new(
            PortErrorKind::Conflict,
            "project registration does not match the selected project",
        ));
    }
    let Some(source) = project.source_path.as_deref().map(PathBuf::from) else {
        return Ok(registration);
    };
    validate_linked_storage_root(&source)?;
    let dicta = source.join(".dicta");
    if !settings.branch_locking {
        return Ok(dicta);
    }
    let branch = dicta_core::git::branch(&source).map_err(|error| {
        PortError::new(
            PortErrorKind::Unavailable,
            format!("could not determine Git branch: {error}"),
        )
    })?;
    let branches = dicta.join("branches");
    ensure_real_directory(&branches, "branch storage")?;
    let branch_path = dicta_core::branch::migrate_legacy_dir(&branches, &branch)
        .map_err(|error| io_port_error("prepare branch storage", &error))?;
    ensure_real_directory(&branch_path, "active branch storage")?;
    let head_oid = dicta_core::git::output(&source, &["rev-parse", "HEAD"])
        .ok()
        .filter(|value| !value.is_empty());
    write_json_atomic(
        &branch_path.join("branch.json"),
        &dicta_core::BranchMetadata {
            git_branch: branch,
            head_oid,
        },
    )
    .map_err(|error| storage_port_error("save branch metadata", &error))?;
    Ok(branch_path)
}

fn project_registration(
    layout: &StorageLayout,
    project_id: &ProjectId,
) -> Result<PathBuf, PortError> {
    let registration = layout.root().join(project_id.as_str());
    let metadata = fs::symlink_metadata(&registration)
        .map_err(|error| io_port_error("inspect project registration", &error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "project registration must be a real directory",
        ));
    }
    if is_symlink(&registration.join("project.json")) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "project registration metadata is symlinked",
        ));
    }
    Ok(registration)
}

fn validate_linked_storage_root(source: &Path) -> Result<(), PortError> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|error| io_port_error("inspect linked project", &error))?;
    if source_metadata.file_type().is_symlink() || !source_metadata.is_dir() {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "linked project must be a real directory",
        ));
    }
    let dicta = source.join(".dicta");
    ensure_real_directory(&dicta, "repository-local Dicta storage")?;
    let canonical_source = source
        .canonicalize()
        .map_err(|error| io_port_error("resolve linked project", &error))?;
    let canonical_dicta = dicta
        .canonicalize()
        .map_err(|error| io_port_error("resolve repository-local Dicta storage", &error))?;
    if !canonical_dicta.starts_with(canonical_source) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "repository-local Dicta storage escaped the linked project",
        ));
    }
    Ok(())
}

fn ensure_real_directory(path: &Path, label: &str) -> Result<(), PortError> {
    if is_symlink(path) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            format!("{label} cannot be a symlink"),
        ));
    }
    fs::create_dir_all(path).map_err(|error| io_port_error(&format!("create {label}"), &error))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_port_error(&format!("inspect {label}"), &error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            format!("{label} must be a real directory"),
        ));
    }
    Ok(())
}

pub struct LinuxStorage<K = SystemClock> {
    layout: StorageLayout,
    clock: K,
    shortcut: OmarchyShortcutIntegration,
    metadata_paths: BTreeMap<RecordingId, PathBuf>,
    retry_scan: Option<RetryScanTask>,
    retry_candidates: VecDeque<RecordingFile>,
}

impl LinuxStorage<SystemClock> {
    #[must_use]
    pub fn system(layout: StorageLayout) -> Self {
        let mut storage = Self::new(layout, SystemClock);
        storage.shortcut = OmarchyShortcutIntegration::discover();
        storage
    }

    #[must_use]
    pub fn with_retry_discovery(mut self) -> Self {
        self.retry_scan = Some(RetryScanTask::spawn(self.layout.clone()));
        self
    }
}

impl<K> LinuxStorage<K> {
    #[must_use]
    pub fn new(layout: StorageLayout, clock: K) -> Self {
        Self {
            layout,
            clock,
            shortcut: OmarchyShortcutIntegration::disabled(),
            metadata_paths: BTreeMap::new(),
            retry_scan: None,
            retry_candidates: VecDeque::new(),
        }
    }

    #[must_use]
    pub const fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    fn registered_projects(&self) -> Result<Vec<(dicta_core::ProjectFile, PathBuf)>, PortError> {
        let entries = fs::read_dir(self.layout.root())
            .map_err(|error| io_port_error("scan storage root", &error))?;
        let mut projects = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| io_port_error("inspect project entry", &error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| io_port_error("inspect project entry type", &error))?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let metadata_path = entry.path().join("project.json");
            let Ok(metadata) = fs::symlink_metadata(&metadata_path) else {
                continue;
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let Ok(project) = read_json::<dicta_core::ProjectFile>(&metadata_path) else {
                continue;
            };
            if project.name.trim().is_empty() {
                continue;
            }
            projects.push((project, entry.path()));
        }
        Ok(projects)
    }

    fn catalog_sources(&self) -> Result<Vec<CatalogSource>, PortError> {
        let mut sources = self
            .registered_projects()?
            .into_iter()
            .map(|(project, registration)| CatalogSource {
                path: project
                    .source_path
                    .as_deref()
                    .map_or(registration, |source| PathBuf::from(source).join(".dicta")),
                project_id: project.id,
                project_name: project.name,
                include_branches: project.source_path.is_some(),
            })
            .collect::<Vec<_>>();
        sources.extend(catalog::general_sources(self.layout.root()));
        catalog::deduplicate_sources(&mut sources);
        Ok(sources)
    }

    fn catalog_recordings(&self) -> Result<Vec<RecordingFile>, PortError> {
        Ok(catalog::load_recordings(&self.catalog_sources()?).recordings)
    }

    fn locate_metadata(&self, recording_id: &RecordingId) -> Result<PathBuf, PortError> {
        if let Some(path) = self.metadata_paths.get(recording_id) {
            return Ok(path.clone());
        }
        let mut found = None;
        for recording in self.catalog_recordings()? {
            if recording.id != *recording_id {
                continue;
            }
            let path = PathBuf::from(&recording.metadata_path);
            if found.replace(path).is_some() {
                return Err(PortError::new(
                    PortErrorKind::Internal,
                    format!("recording ID `{recording_id}` is duplicated in storage"),
                ));
            }
        }
        found.ok_or_else(|| {
            PortError::new(
                PortErrorKind::NotFound,
                format!("recording metadata for `{recording_id}` was not found"),
            )
        })
    }

    fn locate_recording_artifact(
        &self,
        session: &RecordingSession,
        artifact_path: &Path,
    ) -> Result<RecordingLocation, PortError> {
        let metadata = fs::symlink_metadata(artifact_path)
            .map_err(|error| io_port_error("inspect capture artifact", &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "capture artifact must be a regular, non-symlinked file",
            ));
        }
        let expected_name = format!("{}.mp4", session.recording_id);
        if artifact_path.file_name().and_then(|name| name.to_str()) != Some(&expected_name) {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "capture artifact name does not match the recording ID",
            ));
        }
        let artifact = artifact_path
            .canonicalize()
            .map_err(|error| io_port_error("resolve capture artifact", &error))?;
        let Some(project_id) = session
            .project_id
            .as_ref()
            .filter(|id| id.as_str() != dicta_core::GENERAL_PROJECT_ID)
        else {
            let settings = SettingsStore::new(self.layout.root()).load()?;
            let tree =
                storage::general_storage_path(self.layout.root(), settings.general_path.as_deref());
            validate_artifact_tree(&artifact, &tree)?;
            return Ok(RecordingLocation {
                metadata_path: artifact.with_extension("json"),
                scope: RecordingScope::Unprojected,
                git_branch: None,
            });
        };
        let registration = project_registration(&self.layout, project_id)?;
        let project: dicta_core::ProjectFile = read_json(&registration.join("project.json"))
            .map_err(|error| storage_port_error("read project registration", &error))?;
        if project.id != *project_id {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "project registration does not match the recording session",
            ));
        }
        let Some(source) = project.source_path.as_deref().map(PathBuf::from) else {
            validate_artifact_tree(&artifact, &registration)?;
            return Ok(RecordingLocation {
                metadata_path: artifact.with_extension("json"),
                scope: RecordingScope::Repository,
                git_branch: None,
            });
        };
        validate_linked_storage_root(&source)?;
        let dicta = source
            .join(".dicta")
            .canonicalize()
            .map_err(|error| io_port_error("resolve repository-local Dicta storage", &error))?;
        let tree = artifact_tree(&artifact)?;
        if tree == dicta {
            validate_artifact_tree(&artifact, &dicta)?;
            return Ok(RecordingLocation {
                metadata_path: artifact.with_extension("json"),
                scope: RecordingScope::Repository,
                git_branch: None,
            });
        }
        let branches = dicta.join("branches");
        let canonical_branches = branches
            .canonicalize()
            .map_err(|error| io_port_error("resolve branch storage", &error))?;
        if tree.parent() != Some(canonical_branches.as_path()) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "capture artifact is outside the selected project's recording storage",
            ));
        }
        validate_artifact_tree(&artifact, &tree)?;
        let branch_metadata_path = tree.join("branch.json");
        if is_symlink(&branch_metadata_path) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "branch metadata cannot be a symlink",
            ));
        }
        let branch: dicta_core::BranchMetadata = read_json(&branch_metadata_path)
            .map_err(|error| storage_port_error("read branch metadata", &error))?;
        if branch.git_branch.trim().is_empty() {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "branch metadata has no Git branch",
            ));
        }
        Ok(RecordingLocation {
            metadata_path: artifact.with_extension("json"),
            scope: RecordingScope::Branch,
            git_branch: Some(branch.git_branch),
        })
    }
}

fn artifact_tree(artifact: &Path) -> Result<PathBuf, PortError> {
    artifact
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            PortError::new(
                PortErrorKind::PermissionDenied,
                "capture artifact does not have a recording tree",
            )
        })
}

fn validate_artifact_tree(artifact: &Path, tree: &Path) -> Result<(), PortError> {
    let tree = tree
        .canonicalize()
        .map_err(|error| io_port_error("resolve recording storage", &error))?;
    let recordings = tree.join("recordings");
    let recordings = recordings
        .canonicalize()
        .map_err(|error| io_port_error("resolve recordings folder", &error))?;
    if !artifact.starts_with(&recordings) || artifact_tree(artifact)? != tree {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "capture artifact escaped its recording storage",
        ));
    }
    Ok(())
}

type RetryScanResult = Result<VecDeque<RecordingFile>, PortError>;

struct RetryScanTask {
    result: Arc<Mutex<Option<RetryScanResult>>>,
    thread: Option<JoinHandle<()>>,
}

impl RetryScanTask {
    fn spawn(layout: StorageLayout) -> Self {
        let result = Arc::new(Mutex::new(None));
        let task_result = Arc::clone(&result);
        let thread = thread::Builder::new()
            .name("dicta-transcription-retry-scan".to_owned())
            .spawn(move || {
                let mut storage = LinuxStorage::new(layout, SystemClock);
                let discovered = storage
                    .load_recordings()
                    .map(dicta_transcribe::retry_candidates)
                    .map(VecDeque::from);
                *task_result
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(discovered);
            })
            .ok();
        if thread.is_none() {
            *result
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Err(PortError::new(
                PortErrorKind::Internal,
                "could not start transcription retry discovery",
            )));
        }
        Self { result, thread }
    }

    fn poll(&mut self) -> Option<RetryScanResult> {
        self.result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    fn detach_finished(&mut self) {
        let _ = self.thread.take();
    }
}

impl<K> StoragePort for LinuxStorage<K>
where
    K: Clock,
{
    fn load_settings(&mut self) -> Result<storage::AppSettings, PortError> {
        let settings = SettingsStore::new(self.layout.root()).load()?;
        self.shortcut.sync_if_installed(&settings)?;
        Ok(settings)
    }

    fn save_settings(&mut self, settings: &storage::AppSettings) -> Result<(), PortError> {
        let store = SettingsStore::new(self.layout.root());
        let previous = store.load()?;
        self.shortcut.sync_if_installed(settings)?;
        if let Err(error) = store.save(settings) {
            let _ = self.shortcut.sync_if_installed(&previous);
            return Err(error);
        }
        Ok(())
    }

    fn save_timeline_notes(
        &mut self,
        recording: &RecordingFile,
        notes: &[TimelineNote],
    ) -> Result<RecordingFile, PortError> {
        let path = self.locate_metadata(&recording.id)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_port_error("inspect recording metadata", &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "recording metadata must be a regular, non-symlinked file",
            ));
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| io_port_error("resolve recording metadata", &error))?;
        let expected = PathBuf::from(&recording.metadata_path)
            .canonicalize()
            .map_err(|error| io_port_error("resolve catalog recording metadata", &error))?;
        if canonical != expected {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "recording metadata identity changed during timeline-note update",
            ));
        }
        let mut current: RecordingFile = read_json(&canonical)
            .map_err(|error| storage_port_error("read recording metadata", &error))?;
        if current.id != recording.id || current.project_id != recording.project_id {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "recording metadata does not match the timeline-note request",
            ));
        }
        current.timeline_notes = notes.to_vec();
        write_json_atomic(&canonical, &current)
            .map_err(|error| storage_port_error("save timeline notes", &error))?;
        self.metadata_paths.insert(current.id.clone(), canonical);
        Ok(current)
    }

    fn cleanup_merged_videos(
        &mut self,
        project_id: &ProjectId,
    ) -> Result<dicta_control::CleanupSummary, PortError> {
        let settings = SettingsStore::new(self.layout.root()).load()?;
        if !settings.cleanup_merged_videos {
            return Ok(dicta_control::CleanupSummary {
                message: "Merged-video cleanup is off.".to_owned(),
                ..dicta_control::CleanupSummary::default()
            });
        }
        let registration = project_registration(&self.layout, project_id)?;
        let project: dicta_core::ProjectFile = read_json(&registration.join("project.json"))
            .map_err(|error| storage_port_error("read project registration", &error))?;
        if project.id != *project_id {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "project registration does not match the cleanup request",
            ));
        }
        let source = project
            .source_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::InvalidRequest,
                    "merged-video cleanup requires a linked Git project",
                )
            })?;
        validate_linked_storage_root(&source)?;
        let default_branch = default_git_branch(&source).ok_or_else(|| {
            PortError::new(
                PortErrorKind::Unavailable,
                "could not determine the repository default branch",
            )
        })?;
        let active_branch = dicta_core::git::branch(&source).ok();
        let branches = source.join(".dicta/branches");
        if is_symlink(&branches) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "branch storage cannot be a symlink",
            ));
        }
        clean_merged_branch_packets(
            &source,
            &branches,
            &default_branch,
            active_branch.as_deref(),
        )
    }

    fn load_projects(&mut self) -> Result<Vec<dicta_core::ProjectFile>, PortError> {
        let mut projects = self
            .registered_projects()?
            .into_iter()
            .map(|(project, _registration)| project)
            .collect::<Vec<_>>();
        let general_id = ProjectId::new(dicta_core::GENERAL_PROJECT_ID).map_err(|error| {
            PortError::new(
                PortErrorKind::Internal,
                format!("core general project ID is invalid: {error}"),
            )
        })?;
        if !projects.iter().any(|project| project.id == general_id) {
            let settings =
                read_json::<storage::GeneralSettings>(&self.layout.root().join("settings.json"))
                    .unwrap_or_default();
            let general_path =
                storage::general_storage_path(self.layout.root(), settings.general_path.as_deref());
            projects.push(dicta_core::ProjectFile {
                id: general_id,
                name: "General".to_owned(),
                created_at: std::time::UNIX_EPOCH.into(),
                source_path: Some(general_path.to_string_lossy().into_owned()),
                extra: serde_json::Map::new(),
            });
        }
        Ok(projects)
    }

    fn load_recordings(&mut self) -> Result<Vec<RecordingFile>, PortError> {
        let recordings = self.catalog_recordings()?;
        let mut locations = BTreeMap::<RecordingId, Vec<PathBuf>>::new();
        for recording in &recordings {
            locations
                .entry(recording.id.clone())
                .or_default()
                .push(PathBuf::from(&recording.metadata_path));
        }
        for (recording_id, paths) in locations {
            if let [path] = paths.as_slice() {
                self.metadata_paths.insert(recording_id, path.clone());
            } else {
                self.metadata_paths.remove(&recording_id);
            }
        }
        Ok(recordings)
    }

    fn poll_transcription_retry(&mut self) -> Option<Result<RecordingFile, PortError>> {
        if let Some(recording) = self.retry_candidates.pop_front() {
            return Some(Ok(recording));
        }
        let discovered = self.retry_scan.as_mut()?.poll()?;
        if let Some(mut scan) = self.retry_scan.take() {
            scan.detach_finished();
        }
        match discovered {
            Ok(mut candidates) => {
                let mut counts = BTreeMap::<RecordingId, usize>::new();
                for recording in &candidates {
                    *counts.entry(recording.id.clone()).or_default() += 1;
                }
                candidates.retain(|recording| counts.get(&recording.id) == Some(&1));
                for recording in &candidates {
                    self.metadata_paths.insert(
                        recording.id.clone(),
                        PathBuf::from(&recording.metadata_path),
                    );
                }
                self.retry_candidates = candidates;
                self.retry_candidates.pop_front().map(Ok)
            }
            Err(error) => Some(Err(error)),
        }
    }

    fn add_project(
        &mut self,
        path: &str,
        name: Option<&str>,
    ) -> Result<dicta_core::ProjectFile, PortError> {
        let selected = PathBuf::from(path);
        let metadata = fs::symlink_metadata(&selected)
            .map_err(|error| io_port_error("inspect selected project", &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(PortError::new(
                PortErrorKind::InvalidRequest,
                "choose an existing, non-symlinked project directory",
            ));
        }
        let source = dicta_core::git::root(&selected).map_err(|error| {
            PortError::new(
                PortErrorKind::NotFound,
                format!("Git root validation failed: {error}"),
            )
        })?;
        let source_text = source.to_string_lossy().into_owned();
        for (existing, _registration) in self.registered_projects()? {
            if existing
                .source_path
                .as_deref()
                .is_some_and(|candidate| paths_match(candidate, &source))
            {
                prepare_linked_project(&source, &existing)?;
                return Ok(existing);
            }
        }
        let project_name = validated_project_name(name.unwrap_or_else(|| {
            source
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("project")
        }))?;
        let base = slugify(&project_name);
        let mut hasher = DefaultHasher::new();
        source_text.hash(&mut hasher);
        let suffix = format!("{:06x}", hasher.finish() & 0x00ff_ffff);
        let created_at = DateTime::<Utc>::from(self.clock.now());
        for attempt in 0..100_u16 {
            let candidate = if attempt == 0 {
                base.clone()
            } else if attempt == 1 {
                format!("{base}-{suffix}")
            } else {
                format!("{base}-{suffix}-{attempt}")
            };
            let id = ProjectId::new(candidate).map_err(|error| {
                PortError::new(
                    PortErrorKind::Internal,
                    format!("project ID failed: {error}"),
                )
            })?;
            let registration = self.layout.root().join(id.as_str());
            match fs::create_dir(&registration) {
                Ok(()) => {
                    let project = dicta_core::ProjectFile {
                        id,
                        name: project_name.clone(),
                        created_at,
                        source_path: Some(source_text.clone()),
                        extra: serde_json::Map::new(),
                    };
                    if let Err(error) = prepare_linked_project(&source, &project).and_then(|()| {
                        write_json_atomic(&registration.join("project.json"), &project).map_err(
                            |error| storage_port_error("save project registration", &error),
                        )
                    }) {
                        let _ = fs::remove_dir_all(&registration);
                        return Err(error);
                    }
                    return Ok(project);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_port_error("reserve project registration", &error)),
            }
        }
        Err(PortError::new(
            PortErrorKind::Conflict,
            "could not reserve a collision-free project registration",
        ))
    }

    fn create_project(&mut self, name: &str) -> Result<dicta_core::ProjectFile, PortError> {
        let name = validated_project_name(name)?;
        let created_at = DateTime::<Utc>::from(self.clock.now());
        let base = slugify(&name);
        let timestamp = created_at.format("%y%m%d%H%M%S");
        for attempt in 0..100_u16 {
            let id = ProjectId::new(format!(
                "{base}-{timestamp}-{}-{attempt}",
                std::process::id()
            ))
            .map_err(|error| PortError::new(PortErrorKind::Internal, error.to_string()))?;
            let registration = self.layout.root().join(id.as_str());
            match fs::create_dir(&registration) {
                Ok(()) => {
                    let project = dicta_core::ProjectFile {
                        id,
                        name: name.clone(),
                        created_at,
                        source_path: None,
                        extra: serde_json::Map::new(),
                    };
                    let result = fs::create_dir(registration.join("recordings"))
                        .map_err(|error| io_port_error("create project recordings", &error))
                        .and_then(|()| {
                            write_json_atomic(&registration.join("project.json"), &project).map_err(
                                |error| storage_port_error("save project metadata", &error),
                            )
                        });
                    if let Err(error) = result {
                        let _ = fs::remove_dir_all(&registration);
                        return Err(error);
                    }
                    return Ok(project);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_port_error("reserve project directory", &error)),
            }
        }
        Err(PortError::new(
            PortErrorKind::Conflict,
            "could not reserve a collision-free project directory",
        ))
    }

    fn remove_project(&mut self, project_id: &ProjectId) -> Result<(), PortError> {
        if project_id.as_str() == dicta_core::GENERAL_PROJECT_ID {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "General cannot be removed",
            ));
        }
        let registration = self.layout.root().join(project_id.as_str());
        let directory = fs::symlink_metadata(&registration)
            .map_err(|error| io_port_error("inspect project registration", &error))?;
        if directory.file_type().is_symlink() || !directory.is_dir() {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "project registration is not a real directory",
            ));
        }
        let metadata_path = registration.join("project.json");
        if is_symlink(&metadata_path) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "project registration metadata is symlinked",
            ));
        }
        let project: dicta_core::ProjectFile = read_json(&metadata_path)
            .map_err(|error| storage_port_error("read project registration", &error))?;
        if project.id != *project_id {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                "project registration does not match the requested project",
            ));
        }
        let timestamp = DateTime::<Utc>::from(self.clock.now()).timestamp_micros();
        for attempt in 0..100_u16 {
            let archived = registration.join(format!(
                "project.removed-{timestamp}-{}-{attempt}.json",
                std::process::id()
            ));
            if archived.exists() {
                continue;
            }
            fs::rename(&metadata_path, archived)
                .map_err(|error| io_port_error("archive project registration", &error))?;
            return Ok(());
        }
        Err(PortError::new(
            PortErrorKind::Conflict,
            "could not reserve a project registration archive",
        ))
    }

    fn delete_recording(&mut self, recording: &RecordingFile) -> Result<(), PortError> {
        let matches = self
            .catalog_recordings()?
            .into_iter()
            .filter(|candidate| {
                candidate.id == recording.id && candidate.project_id == recording.project_id
            })
            .collect::<Vec<_>>();
        let [located] = matches.as_slice() else {
            let message = if matches.is_empty() {
                format!("recording {} was not found", recording.id)
            } else {
                format!(
                    "recording {} is duplicated within project {}",
                    recording.id, recording.project_id
                )
            };
            return Err(PortError::new(
                if matches.is_empty() {
                    PortErrorKind::NotFound
                } else {
                    PortErrorKind::Internal
                },
                message,
            ));
        };
        let metadata_path = PathBuf::from(&located.metadata_path);
        let canonical_metadata = metadata_path
            .canonicalize()
            .map_err(|error| io_port_error("resolve recording metadata", &error))?;
        let mut recordings_root = None;
        for source in self.catalog_sources()? {
            for tree in catalog::recording_trees(&source) {
                let Ok(root) = tree.join("recordings").canonicalize() else {
                    continue;
                };
                if canonical_metadata.starts_with(&root) {
                    if source.project_id != located.project_id {
                        return Err(PortError::new(
                            PortErrorKind::PermissionDenied,
                            "recording metadata resolved beneath another project's storage",
                        ));
                    }
                    recordings_root = Some(root);
                    break;
                }
            }
        }
        let recordings_root = recordings_root.ok_or_else(|| {
            PortError::new(
                PortErrorKind::PermissionDenied,
                "recording metadata is outside every registered recordings root",
            )
        })?;
        let artifacts = recording_artifact_paths(&canonical_metadata);
        for artifact in &artifacts {
            let metadata = match fs::symlink_metadata(artifact) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(io_port_error("inspect recording artifact", &error)),
            };
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(PortError::new(
                    PortErrorKind::PermissionDenied,
                    format!(
                        "refusing non-regular recording artifact {}",
                        artifact.display()
                    ),
                ));
            }
            let canonical = artifact
                .canonicalize()
                .map_err(|error| io_port_error("resolve recording artifact", &error))?;
            if !canonical.starts_with(&recordings_root) {
                return Err(PortError::new(
                    PortErrorKind::PermissionDenied,
                    format!(
                        "refusing recording artifact outside its catalog root: {}",
                        artifact.display()
                    ),
                ));
            }
        }
        for artifact in artifacts {
            match fs::remove_file(&artifact) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_port_error("delete recording artifact", &error)),
            }
        }
        self.metadata_paths.remove(&recording.id);
        if let Some(day) = canonical_metadata.parent() {
            if fs::read_dir(day).is_ok_and(|mut entries| entries.next().is_none()) {
                let _ = fs::remove_dir(day);
            }
        }
        Ok(())
    }

    fn save_recording(
        &mut self,
        session: &RecordingSession,
        artifact: &CaptureArtifact,
        annotations: Option<&AnnotationFile>,
    ) -> Result<RecordingFile, PortError> {
        let location = self.locate_recording_artifact(session, &artifact.path)?;
        let size_bytes = fs::metadata(&artifact.path)
            .map_err(|error| io_port_error("inspect capture artifact", &error))?
            .len();
        let metadata_path = location.metadata_path.clone();
        if metadata_path.exists() {
            return Err(PortError::new(
                PortErrorKind::Internal,
                format!(
                    "refusing to replace recording metadata {}",
                    metadata_path.display()
                ),
            ));
        }
        if annotations.is_some_and(|document| {
            !document.is_valid() || document.recording_id != session.recording_id
        }) {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "annotation document does not match the recording",
            ));
        }

        let annotation_path = annotations.map(|document| {
            let path = annotation_sidecar_path(&metadata_path);
            write_json_atomic(&path, document)
                .map_err(|error| storage_port_error("save annotation sidecar", &error))?;
            Ok::<String, PortError>(path.to_string_lossy().into_owned())
        });
        let annotation_path = annotation_path.transpose()?;

        let recording = recording_model(
            session,
            artifact,
            &metadata_path,
            annotation_path,
            size_bytes,
            self.clock.now(),
            &location,
        )?;
        write_json_atomic(&metadata_path, &recording)
            .map_err(|error| storage_port_error("save recording metadata", &error))?;
        self.metadata_paths
            .insert(session.recording_id.clone(), metadata_path);
        Ok(recording)
    }

    fn save_transcription(
        &mut self,
        recording_id: &RecordingId,
        output: &TranscriptionOutput,
    ) -> Result<(), PortError> {
        if output.transcript.trim().is_empty()
            || !output
                .segments
                .iter()
                .all(dicta_core::TranscriptSegment::is_valid)
        {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "transcription output is empty or has invalid segments",
            ));
        }
        let path = self.locate_metadata(recording_id)?;
        let mut recording: RecordingFile = read_json(&path)
            .map_err(|error| storage_port_error("read recording metadata", &error))?;
        if recording.id != *recording_id {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "recording metadata ID does not match the requested transcription",
            ));
        }
        recording.transcript = Some(output.transcript.clone());
        recording.transcript_segments.clone_from(&output.segments);
        recording
            .transcription_language
            .clone_from(&output.detected_language);
        recording.transcription_status = TranscriptionStatus::Complete;
        recording.transcription_error = None;
        write_json_atomic(&path, &recording)
            .map_err(|error| storage_port_error("save transcription metadata", &error))
    }

    fn mark_transcription_pending(&mut self, recording_id: &RecordingId) -> Result<(), PortError> {
        let path = self.locate_metadata(recording_id)?;
        let mut recording: RecordingFile = read_json(&path)
            .map_err(|error| storage_port_error("read recording metadata", &error))?;
        if recording.id != *recording_id {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "recording metadata ID does not match the pending transcription",
            ));
        }
        recording.transcription_status = TranscriptionStatus::Pending;
        recording.transcription_error = None;
        write_json_atomic(&path, &recording)
            .map_err(|error| storage_port_error("save pending transcription state", &error))
    }

    fn mark_transcription_failed(
        &mut self,
        recording_id: &RecordingId,
        message: &str,
    ) -> Result<(), PortError> {
        let path = self.locate_metadata(recording_id)?;
        let mut recording: RecordingFile = read_json(&path)
            .map_err(|error| storage_port_error("read recording metadata", &error))?;
        if recording.id != *recording_id {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "recording metadata ID does not match the failed transcription",
            ));
        }
        recording.transcription_status = TranscriptionStatus::Failed;
        recording.transcription_error = Some(if message.trim().is_empty() {
            "transcription failed".to_owned()
        } else {
            message.trim().to_owned()
        });
        write_json_atomic(&path, &recording)
            .map_err(|error| storage_port_error("save failed transcription state", &error))
    }
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

fn recording_artifact_paths(metadata_path: &Path) -> Vec<PathBuf> {
    let Some(parent) = metadata_path.parent() else {
        return vec![metadata_path.to_path_buf()];
    };
    let Some(stem) = metadata_path.file_stem().and_then(|value| value.to_str()) else {
        return vec![metadata_path.to_path_buf()];
    };
    [
        format!("{stem}.mp4"),
        format!("{stem}.poster.jpg"),
        format!("{stem}.transcript.md"),
        format!("{stem}.transcript.base.md"),
        format!("{stem}.transcript.json"),
        format!("{stem}.md"),
        format!("{stem}.annotations.json"),
        format!("{stem}.json"),
    ]
    .into_iter()
    .map(|name| parent.join(name))
    .collect()
}

fn git_ref_exists(source: &Path, reference: &str) -> bool {
    dicta_core::git::output(source, &["rev-parse", "--verify", "--quiet", reference]).is_ok()
}

fn default_git_branch(source: &Path) -> Option<String> {
    if let Ok(remote) = dicta_core::git::output(
        source,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        if let Some(branch) = remote
            .strip_prefix("origin/")
            .filter(|branch| !branch.is_empty())
        {
            return Some(branch.to_owned());
        }
    }
    ["main", "master"]
        .into_iter()
        .find(|branch| git_ref_exists(source, &format!("refs/heads/{branch}")))
        .map(str::to_owned)
}

fn revision_is_merged(source: &Path, revision: &str, default_branch: &str) -> bool {
    dicta_core::git::output(
        source,
        &["merge-base", "--is-ancestor", revision, default_branch],
    )
    .is_ok()
}

fn clean_merged_branch_packets(
    source: &Path,
    branches: &Path,
    default_branch: &str,
    active_branch: Option<&str>,
) -> Result<dicta_control::CleanupSummary, PortError> {
    let entries = match fs::read_dir(branches) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(dicta_control::CleanupSummary {
                default_branch: Some(default_branch.to_owned()),
                message: "No branch recordings to clean.".to_owned(),
                ..dicta_control::CleanupSummary::default()
            });
        }
        Err(error) => return Err(io_port_error("inspect branch recordings", &error)),
    };
    let canonical_branches = branches
        .canonicalize()
        .map_err(|error| io_port_error("resolve branch storage", &error))?;
    let mut summary = dicta_control::CleanupSummary {
        default_branch: Some(default_branch.to_owned()),
        ..dicta_control::CleanupSummary::default()
    };
    for entry in entries {
        let entry = entry.map_err(|error| io_port_error("inspect branch entry", &error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| io_port_error("inspect branch entry type", &error))?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let branch_path = entry.path();
        let canonical_branch = branch_path
            .canonicalize()
            .map_err(|error| io_port_error("resolve branch packet", &error))?;
        if canonical_branch.parent() != Some(canonical_branches.as_path()) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "branch packet escaped repository-local storage",
            ));
        }
        let metadata_path = branch_path.join("branch.json");
        if is_symlink(&metadata_path) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "branch metadata cannot be a symlink",
            ));
        }
        let Ok(branch) = read_json::<dicta_core::BranchMetadata>(&metadata_path) else {
            continue;
        };
        if branch.git_branch == default_branch || active_branch == Some(branch.git_branch.as_str())
        {
            continue;
        }
        let revision = branch.head_oid.or_else(|| {
            let reference = format!("refs/heads/{}", branch.git_branch);
            git_ref_exists(source, &reference).then_some(reference)
        });
        let Some(revision) = revision else {
            continue;
        };
        if !revision_is_merged(source, &revision, default_branch) {
            continue;
        }
        let (removed, bytes) =
            remove_video_files(&branch_path.join("recordings"), &canonical_branch)?;
        if removed > 0 {
            summary.removed_files += removed;
            summary.freed_bytes += bytes;
            summary.cleaned_branches.push(branch.git_branch);
        }
    }
    summary.message = if summary.removed_files == 0 {
        format!("No merged videos found for {default_branch}.")
    } else {
        format!(
            "Removed {} merged video{}.",
            summary.removed_files,
            if summary.removed_files == 1 { "" } else { "s" }
        )
    };
    Ok(summary)
}

fn remove_video_files(directory: &Path, branch_root: &Path) -> Result<(usize, u64), PortError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(io_port_error("inspect branch recordings", &error)),
    };
    let mut removed = 0;
    let mut freed = 0;
    for entry in entries {
        let entry = entry.map_err(|error| io_port_error("inspect recording artifact", &error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| io_port_error("inspect recording artifact type", &error))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let (nested_removed, nested_freed) = remove_video_files(&entry.path(), branch_root)?;
            removed += nested_removed;
            freed += nested_freed;
            continue;
        }
        if !file_type.is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("mp4")
        {
            continue;
        }
        let canonical = entry
            .path()
            .canonicalize()
            .map_err(|error| io_port_error("resolve merged recording", &error))?;
        if !canonical.starts_with(branch_root) {
            return Err(PortError::new(
                PortErrorKind::PermissionDenied,
                "merged recording escaped its branch packet",
            ));
        }
        let bytes = fs::metadata(&canonical)
            .map_err(|error| io_port_error("inspect merged recording", &error))?
            .len();
        fs::remove_file(&canonical)
            .map_err(|error| io_port_error("delete merged recording", &error))?;
        removed += 1;
        freed += bytes;
    }
    Ok((removed, freed))
}

fn validated_project_name(name: &str) -> Result<String, PortError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PortError::new(
            PortErrorKind::InvalidRequest,
            "project name cannot be empty",
        ));
    }
    if name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err(PortError::new(
            PortErrorKind::InvalidRequest,
            "project name is too long or contains control characters",
        ));
    }
    Ok(name.to_owned())
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for character in name.trim().to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "project".to_owned()
    } else {
        slug.to_owned()
    }
}

fn paths_match(candidate: &str, source: &Path) -> bool {
    Path::new(candidate)
        .canonicalize()
        .is_ok_and(|candidate| candidate == source)
}

fn prepare_linked_project(
    source: &Path,
    project: &dicta_core::ProjectFile,
) -> Result<(), PortError> {
    let storage = source.join(".dicta");
    if is_symlink(&storage) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "repository-local .dicta storage cannot be a symlink",
        ));
    }
    fs::create_dir_all(&storage)
        .map_err(|error| io_port_error("create repository-local Dicta storage", &error))?;
    let published = storage.join("project.json");
    if is_symlink(&published) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "repository-local project metadata cannot be a symlink",
        ));
    }
    if published.exists() {
        let existing: dicta_core::ProjectFile = read_json(&published)
            .map_err(|error| storage_port_error("read repository project metadata", &error))?;
        if existing.id != project.id {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                format!("repository is already linked to project {}", existing.id),
            ));
        }
    }
    write_json_atomic(&published, project)
        .map_err(|error| storage_port_error("publish repository project metadata", &error))?;
    let branch = dicta_core::git::branch(source).map_err(|error| {
        PortError::new(
            PortErrorKind::Unavailable,
            format!("could not determine Git branch: {error}"),
        )
    })?;
    let branches = storage.join("branches");
    let branch_path = dicta_core::branch::migrate_legacy_dir(&branches, &branch)
        .map_err(|error| io_port_error("prepare branch storage", &error))?;
    if is_symlink(&branch_path) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "branch storage cannot be a symlink",
        ));
    }
    fs::create_dir_all(branch_path.join("recordings"))
        .map_err(|error| io_port_error("create branch recordings", &error))?;
    let head_oid = dicta_core::git::output(source, &["rev-parse", "HEAD"])
        .ok()
        .filter(|value| !value.is_empty());
    let branch_metadata = dicta_core::BranchMetadata {
        git_branch: branch,
        head_oid,
    };
    write_json_atomic(&branch_path.join("branch.json"), &branch_metadata)
        .map_err(|error| storage_port_error("save branch metadata", &error))?;
    exclude_dicta_from_git(source)
}

fn exclude_dicta_from_git(source: &Path) -> Result<(), PortError> {
    let administration = dicta_core::git::admin_paths(source).map_err(|error| {
        PortError::new(
            PortErrorKind::PermissionDenied,
            format!("Git administration validation failed: {error}"),
        )
    })?;
    let info = administration.common.join("info");
    if is_symlink(&info) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "local .git/info cannot be a symlink",
        ));
    }
    fs::create_dir_all(&info).map_err(|error| io_port_error("create .git/info", &error))?;
    let exclude = info.join("exclude");
    if is_symlink(&exclude) {
        return Err(PortError::new(
            PortErrorKind::PermissionDenied,
            "local Git exclude file cannot be a symlink",
        ));
    }
    let existing = match fs::read_to_string(&exclude) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io_port_error("read local Git exclude", &error)),
    };
    if existing.lines().any(|line| line.trim() == ".dicta/") {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .map_err(|error| io_port_error("open local Git exclude", &error))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")
            .map_err(|error| io_port_error("update local Git exclude", &error))?;
    }
    file.write_all(b".dicta/\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| io_port_error("update local Git exclude", &error))
}

fn recording_model(
    session: &RecordingSession,
    artifact: &CaptureArtifact,
    metadata_path: &std::path::Path,
    annotation_path: Option<String>,
    size_bytes: u64,
    ended_system: std::time::SystemTime,
    location: &RecordingLocation,
) -> Result<RecordingFile, PortError> {
    let started_system = ended_system.checked_sub(artifact.duration);
    let project_id = match &session.project_id {
        Some(project) => project.clone(),
        None => ProjectId::new(dicta_core::GENERAL_PROJECT_ID).map_err(|error| {
            PortError::new(
                PortErrorKind::Internal,
                format!("core general project ID is invalid: {error}"),
            )
        })?,
    };
    let mut extra = Map::new();
    extra.insert(
        "capture_backend".to_owned(),
        json!(match artifact.backend {
            CaptureBackend::GpuScreenRecorder => "gpu-screen-recorder",
            CaptureBackend::WfRecorder => "wf-recorder",
        }),
    );
    extra.insert("capture_output".to_owned(), json!(artifact.output_name));
    extra.insert(
        "capture_geometry".to_owned(),
        json!({
            "x": artifact.geometry.x,
            "y": artifact.geometry.y,
            "width": artifact.geometry.width,
            "height": artifact.geometry.height,
        }),
    );
    extra.insert(
        "capture_scale_milli".to_owned(),
        json!(artifact.scale_milli),
    );
    extra.insert(
        "encoded_pixel_size".to_owned(),
        json!([artifact.encoded_pixel_size.0, artifact.encoded_pixel_size.1]),
    );
    Ok(RecordingFile {
        id: session.recording_id.clone(),
        project_id,
        video_path: artifact.path.to_string_lossy().into_owned(),
        metadata_path: metadata_path.to_string_lossy().into_owned(),
        note: session.note.clone().unwrap_or_default(),
        recording_scope: location.scope,
        git_branch: location.git_branch.clone(),
        started_at: started_system.map(DateTime::<Utc>::from),
        ended_at: Some(DateTime::<Utc>::from(ended_system)),
        duration_seconds: Some(artifact.duration.as_secs_f64()),
        size_bytes: Some(size_bytes),
        success: true,
        transcript: None,
        transcript_path: None,
        transcript_segments: Vec::new(),
        transcription_status: TranscriptionStatus::Unknown(String::new()),
        transcription_error: None,
        transcription_language: None,
        poster_path: poster::extract(&artifact.path)
            .map(|path| path.to_string_lossy().into_owned()),
        annotation_path,
        timeline_notes: Vec::new(),
        extra,
    })
}

fn storage_port_error(action: &str, error: &str) -> PortError {
    PortError::new(
        PortErrorKind::Internal,
        format!("could not {action}: {error}"),
    )
}

fn io_port_error(action: &str, error: &io::Error) -> PortError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => PortErrorKind::PermissionDenied,
        io::ErrorKind::NotFound => PortErrorKind::NotFound,
        _ => PortErrorKind::Internal,
    };
    PortError::new(kind, format!("could not {action}: {error}"))
}
