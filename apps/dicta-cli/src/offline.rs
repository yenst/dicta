use crate::{ClientFailure, FailureKind};
use dicta_control::{
    protocol::{RecordingDocument, RecordingSummary, Response, TranscriptionState},
    Command, RecordingSelector,
};
use dicta_core::{catalog, storage, RecordingFile, TranscriptionStatus};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub enum OfflinePayload {
    Response(Response),
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
        let mut sources = catalog::registered_sources(root);
        sources.extend(catalog::repository_local_sources(&self.working_directory));
        sources.extend(catalog::general_sources(root));
        if let Some(filter) = project_filter {
            sources.retain(|source| {
                source.project_id.as_str() == filter
                    || source.project_name.eq_ignore_ascii_case(filter)
            });
        }
        catalog::deduplicate_sources(&mut sources);
        let names = sources
            .iter()
            .map(|source| (source.project_id.clone(), source.project_name.clone()))
            .collect::<HashMap<_, _>>();
        let loaded = catalog::load_recordings(&sources);
        Ok(LoadReport {
            recordings: loaded
                .recordings
                .into_iter()
                .map(|recording| LoadedRecording {
                    project_name: names
                        .get(&recording.project_id)
                        .cloned()
                        .unwrap_or_else(|| recording.project_id.to_string()),
                    recording,
                })
                .collect(),
            warnings: loaded.warnings,
        })
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
                    payload: OfflinePayload::Response(Response::RecordingDetails(Box::new(
                        recording_document(&selected.recording)?,
                    ))),
                    warnings: std::mem::take(&mut report.warnings),
                }))
            }
            Command::Context {
                recording, project, ..
            } => {
                let mut report = self.load(project.as_deref())?;
                let selected = select(&report.recordings, recording)?;
                Ok(Some(OfflineRead {
                    payload: OfflinePayload::Response(Response::Context {
                        text: render_context(selected),
                    }),
                    warnings: std::mem::take(&mut report.warnings),
                }))
            }
            _ => Ok(None),
        }
    }
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

fn recording_document(recording: &RecordingFile) -> Result<RecordingDocument, ClientFailure> {
    let value = serde_json::to_value(recording).map_err(|error| {
        ClientFailure::new(
            FailureKind::Software,
            format!("could not encode offline recording details: {error}"),
        )
    })?;
    serde_json::from_value(value).map_err(|error| {
        ClientFailure::new(
            FailureKind::Software,
            format!("offline recording details did not match the control document: {error}"),
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
            TranscriptionStatus::Unknown(_) => TranscriptionState::Unavailable,
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
