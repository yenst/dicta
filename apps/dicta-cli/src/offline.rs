use crate::{ClientFailure, FailureKind};
use dicta_control::{
    protocol::{RecordingSummary, Response, TranscriptionState},
    Command, RecordingSelector,
};
use dicta_core::{storage, ProjectFile, RecordingFile, TranscriptionStatus, GENERAL_PROJECT_ID};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub enum OfflinePayload {
    Response(Response),
    Recording(Box<RecordingFile>),
    Context(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct OfflineRead {
    pub payload: OfflinePayload,
    pub warnings: Vec<String>,
}

pub trait OfflineStore {
    /// Returns `None` when a command must be handled by the native process.
    fn read(&self, command: &Command) -> Result<Option<OfflineRead>, ClientFailure>;
}

#[derive(Clone, Debug)]
pub struct FileOfflineStore {
    storage_root: Option<PathBuf>,
    working_directory: PathBuf,
}

impl FileOfflineStore {
    pub fn discover() -> Self {
        let storage_root = env::var_os("DICTA_HOME").map(PathBuf::from).or_else(|| {
            env::var_os("HOME")
                .map(|home| storage::preferred_storage_root(&PathBuf::from(home).join("Documents")))
        });
        Self {
            storage_root,
            working_directory: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn at(storage_root: impl Into<PathBuf>, working_directory: impl Into<PathBuf>) -> Self {
        Self {
            storage_root: Some(storage_root.into()),
            working_directory: working_directory.into(),
        }
    }

    fn root(&self) -> Result<&Path, ClientFailure> {
        self.storage_root.as_deref().ok_or_else(|| {
            ClientFailure::new(
                FailureKind::Unavailable,
                "offline storage could not be discovered; set DICTA_HOME",
            )
        })
    }

    fn load(&self, project_filter: Option<&str>) -> Result<LoadReport, ClientFailure> {
        let root = self.root()?;
        let mut sources = registered_sources(root);
        sources.extend(repository_local_source(&self.working_directory));
        sources.extend(general_sources(root));
        if let Some(filter) = project_filter {
            sources.retain(|source| {
                source.project_id == filter || source.project_name.eq_ignore_ascii_case(filter)
            });
        }
        deduplicate_sources(&mut sources);

        let mut report = LoadReport::default();
        for source in sources {
            load_source(&source, &mut report);
        }
        report.recordings.sort_by(|left, right| {
            right
                .recording
                .started_at
                .cmp(&left.recording.started_at)
                .then_with(|| right.recording.id.as_str().cmp(left.recording.id.as_str()))
        });
        let mut seen = HashSet::new();
        report.recordings.retain(|item| {
            seen.insert((
                item.recording.project_id.clone(),
                item.recording.id.clone(),
                item.recording.metadata_path.clone(),
            ))
        });
        Ok(report)
    }
}

impl OfflineStore for FileOfflineStore {
    fn read(&self, command: &Command) -> Result<Option<OfflineRead>, ClientFailure> {
        match command {
            Command::RecordingList {
                project,
                branch,
                limit,
            } => {
                let mut report = self.load(project.as_deref())?;
                if let Some(branch) = branch {
                    report.recordings.retain(|item| {
                        item.recording.recording_scope == dicta_core::RecordingScope::Repository
                            || item.recording.git_branch.as_deref() == Some(branch)
                    });
                }
                if let Some(limit) = limit {
                    report.recordings.truncate(*limit as usize);
                }
                Ok(Some(OfflineRead {
                    payload: OfflinePayload::Response(Response::Recordings(
                        report.recordings.iter().map(summary).collect(),
                    )),
                    warnings: report.warnings,
                }))
            }
            Command::RecordingShow { recording } => {
                let mut report = self.load(None)?;
                let selected = select(&report.recordings, recording)?;
                Ok(Some(OfflineRead {
                    payload: OfflinePayload::Recording(Box::new(selected.recording.clone())),
                    warnings: std::mem::take(&mut report.warnings),
                }))
            }
            Command::Context {
                recording, project, ..
            } => {
                let mut report = self.load(project.as_deref())?;
                let selected = select(&report.recordings, recording)?;
                Ok(Some(OfflineRead {
                    payload: OfflinePayload::Context(render_context(selected)),
                    warnings: std::mem::take(&mut report.warnings),
                }))
            }
            _ => Ok(None),
        }
    }
}

#[derive(Clone, Debug)]
struct Source {
    path: PathBuf,
    project_id: String,
    project_name: String,
    include_branches: bool,
}

#[derive(Clone, Debug)]
struct LoadedRecording {
    recording: RecordingFile,
    project_name: String,
}

#[derive(Default)]
struct LoadReport {
    recordings: Vec<LoadedRecording>,
    warnings: Vec<String>,
}

fn registered_sources(root: &Path) -> Vec<Source> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return None;
            }
            let metadata_path = entry.path().join("project.json");
            if is_symlink(&metadata_path) {
                return None;
            }
            let project = storage::read_json::<ProjectFile>(&metadata_path).ok()?;
            let path = project
                .source_path
                .as_deref()
                .map(|source| PathBuf::from(source).join(".dicta"))
                .unwrap_or_else(|| entry.path());
            Some(Source {
                path,
                project_id: project.id.to_string(),
                project_name: project.name,
                include_branches: project.source_path.is_some(),
            })
        })
        .collect()
}

fn repository_local_source(working_directory: &Path) -> Vec<Source> {
    let Ok(repo_root) = dicta_core::git::root(working_directory) else {
        return Vec::new();
    };
    let storage_path = repo_root.join(".dicta");
    let project_path = storage_path.join("project.json");
    if is_symlink(&storage_path) || is_symlink(&project_path) {
        return Vec::new();
    }
    let Ok(project) = storage::read_json::<ProjectFile>(&project_path) else {
        return Vec::new();
    };
    vec![Source {
        path: storage_path,
        project_id: project.id.to_string(),
        project_name: project.name,
        include_branches: true,
    }]
}

fn general_sources(root: &Path) -> Vec<Source> {
    let settings = storage::read_json::<storage::GeneralSettings>(&root.join("settings.json"))
        .unwrap_or_default();
    storage::general_storage_candidates(root, settings.general_path.as_deref())
        .into_iter()
        .map(|path| Source {
            path,
            project_id: GENERAL_PROJECT_ID.to_string(),
            project_name: "General".to_string(),
            include_branches: false,
        })
        .collect()
}

fn deduplicate_sources(sources: &mut Vec<Source>) {
    let mut seen = HashSet::new();
    sources.retain(|source| {
        let key = source
            .path
            .canonicalize()
            .unwrap_or_else(|_| source.path.clone());
        seen.insert((key, source.project_id.clone()))
    });
}

fn load_source(source: &Source, report: &mut LoadReport) {
    load_recording_tree(source, &source.path, report);
    if !source.include_branches {
        return;
    }
    let branches = source.path.join("branches");
    let Ok(entries) = fs::read_dir(&branches) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && !kind.is_symlink() {
            load_recording_tree(source, &entry.path(), report);
        } else if kind.is_symlink() {
            report.warnings.push(format!(
                "ignored symlinked branch storage `{}`",
                entry.path().display()
            ));
        }
    }
}

fn load_recording_tree(source: &Source, tree: &Path, report: &mut LoadReport) {
    if is_symlink(tree) {
        report
            .warnings
            .push(format!("ignored symlinked storage `{}`", tree.display()));
        return;
    }
    let recordings_root = tree.join("recordings");
    if is_symlink(&recordings_root) {
        report.warnings.push(format!(
            "ignored symlinked recordings storage `{}`",
            recordings_root.display()
        ));
        return;
    }
    let Ok(days) = fs::read_dir(&recordings_root) else {
        return;
    };
    for day in days.flatten() {
        let Ok(kind) = day.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            report.warnings.push(format!(
                "ignored symlinked recording day `{}`",
                day.path().display()
            ));
            continue;
        }
        if !kind.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(day.path()) else {
            report
                .warnings
                .push(format!("could not read `{}`", day.path().display()));
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_transcript = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".transcript.json"));
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file()
                && !kind.is_symlink()
                && !is_transcript
                && path.extension().and_then(|value| value.to_str()) == Some("json")
            {
                match read_recording(&path, &recordings_root, source) {
                    Ok(recording) => report.recordings.push(recording),
                    Err(error) => report
                        .warnings
                        .push(format!("ignored `{}`: {error}", path.display())),
                }
            } else if kind.is_symlink() {
                report
                    .warnings
                    .push(format!("ignored symlinked artifact `{}`", path.display()));
            }
        }
    }
}

fn read_recording(
    path: &Path,
    recordings_root: &Path,
    source: &Source,
) -> Result<LoadedRecording, String> {
    let mut recording = storage::read_json::<RecordingFile>(path)?;
    if !recording.is_valid() {
        return Err("recording metadata failed validation".to_string());
    }
    if recording.project_id.as_str() != source.project_id {
        return Err(format!(
            "recording belongs to project `{}` instead of `{}`",
            recording.project_id, source.project_id
        ));
    }
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("could not resolve metadata: {error}"))?;
    let canonical_root = recordings_root
        .canonicalize()
        .map_err(|error| format!("could not resolve recordings root: {error}"))?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err("metadata escaped the recordings root".to_string());
    }
    recording.metadata_path = canonical_path.to_string_lossy().into_owned();
    if recording.transcript.is_none() {
        if let Some((path, transcript)) =
            read_transcript(&recording, &canonical_path, &canonical_root)
        {
            recording.transcript_path = Some(path.to_string_lossy().into_owned());
            recording.transcript = Some(transcript);
        }
    }
    Ok(LoadedRecording {
        recording,
        project_name: source.project_name.clone(),
    })
}

fn read_transcript(
    recording: &RecordingFile,
    metadata_path: &Path,
    recordings_root: &Path,
) -> Option<(PathBuf, String)> {
    let mut candidates = Vec::new();
    if let Some(raw) = recording.transcript_path.as_deref() {
        let path = PathBuf::from(raw);
        candidates.push(if path.is_absolute() {
            path
        } else {
            metadata_path.parent()?.join(path)
        });
    }
    let stem = metadata_path.file_stem()?.to_str()?;
    candidates.push(metadata_path.with_file_name(format!("{stem}.transcript.md")));
    candidates.push(metadata_path.with_file_name(format!("{stem}.md")));
    for candidate in candidates {
        let Ok(metadata) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(recordings_root) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&canonical) {
            return Some((canonical, content));
        }
    }
    None
}

fn select<'a>(
    recordings: &'a [LoadedRecording],
    selector: &RecordingSelector,
) -> Result<&'a LoadedRecording, ClientFailure> {
    let selected = match selector {
        RecordingSelector::Latest => recordings.first(),
        RecordingSelector::Id(id) => {
            let mut matches = recordings
                .iter()
                .filter(|item| item.recording.id.as_str() == id);
            let first = matches.next();
            if let (Some(first), Some(second)) = (first, matches.next()) {
                return Err(ClientFailure::new(
                    FailureKind::Conflict,
                    format!(
                        "recording `{id}` is ambiguous between projects `{}` and `{}`; use `context {id} --project <project>` or narrow the project",
                        first.recording.project_id, second.recording.project_id
                    ),
                ));
            }
            first
        }
    };
    selected.ok_or_else(|| {
        let label = match selector {
            RecordingSelector::Latest => "latest".to_string(),
            RecordingSelector::Id(id) => id.clone(),
        };
        ClientFailure::new(
            FailureKind::NotFound,
            format!("recording `{label}` was not found in offline storage"),
        )
    })
}

fn summary(item: &LoadedRecording) -> RecordingSummary {
    RecordingSummary {
        id: item.recording.id.to_string(),
        project: Some(item.recording.project_id.to_string()),
        branch: item.recording.git_branch.clone(),
        started_at: item.recording.started_at.map(|value| value.to_rfc3339()),
        note: item.recording.note.clone(),
        transcript_preview: item
            .recording
            .transcript
            .as_deref()
            .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(180).collect()),
        success: item.recording.success,
        recording_scope: item.recording.recording_scope.to_string(),
        timeline_note_count: u32::try_from(item.recording.timeline_notes.len()).unwrap_or(u32::MAX),
        has_annotations: item.recording.annotation_path.is_some(),
        duration_seconds: item.recording.duration_seconds.unwrap_or(0.0),
        transcription: match item.recording.transcription_status {
            TranscriptionStatus::Pending => TranscriptionState::Pending,
            TranscriptionStatus::Processing => TranscriptionState::Processing,
            TranscriptionStatus::Complete => TranscriptionState::Complete,
            TranscriptionStatus::Failed => TranscriptionState::Failed,
            TranscriptionStatus::Unknown => TranscriptionState::Unavailable,
        },
    }
}

const CONTEXT_TRANSCRIPT_LIMIT: usize = 1_200;

fn transcript_excerpt(transcript: &str) -> String {
    let transcript = transcript.split_whitespace().collect::<Vec<_>>().join(" ");
    if transcript.chars().count() <= CONTEXT_TRANSCRIPT_LIMIT {
        return transcript;
    }
    let mut excerpt = transcript
        .chars()
        .take(CONTEXT_TRANSCRIPT_LIMIT)
        .collect::<String>();
    if let Some(boundary) = excerpt.rfind(char::is_whitespace) {
        excerpt.truncate(boundary);
    }
    excerpt.push_str("…\n\n_(Transcript truncated; open the recording for the full text.)_");
    excerpt
}

fn render_context(item: &LoadedRecording) -> String {
    let recording = &item.recording;
    let mut output = format!(
        "# Dicta recording: {}\n\nProject: {} (`{}`)\n",
        recording.id, item.project_name, recording.project_id
    );
    if let Some(branch) = recording.git_branch.as_deref() {
        output.push_str(&format!("Branch: `{branch}`\n"));
    }
    if !recording.note.trim().is_empty() {
        output.push_str(&format!("\n## Note\n\n{}\n", recording.note.trim()));
    }
    if let Some(transcript) = recording.transcript.as_deref() {
        output.push_str(&format!(
            "\n## Transcript excerpt\n\n{}\n",
            transcript_excerpt(transcript)
        ));
    } else if !recording.transcript_segments.is_empty() {
        let mut transcript = String::new();
        for segment in &recording.transcript_segments {
            transcript.push_str(&format!(
                "[{}] {} ",
                dicta_core::transcript::format_timestamp(segment.start_seconds),
                segment.text.trim()
            ));
        }
        output.push_str(&format!(
            "\n## Transcript excerpt\n\n{}\n",
            transcript_excerpt(&transcript)
        ));
    } else {
        output.push_str("\nTranscript unavailable.\n");
    }
    output
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}
