//! Control-protocol command dispatch for [`Runtime`].

use super::{
    render::{
        model_status_summary, project_summary, recording_summary, render_recording_context,
        resolve_recording_from, sort_recordings_latest_first, validate_general_path,
        validate_timeline_notes,
    },
    session_from_state,
    wire::{recording_document, settings_document, timeline_notes_from_wire},
    AnnotationEdit, Runtime,
};
use crate::{
    error::RuntimeError,
    ports::{
        AnnotationPort, CapturePort, Clock, Completion, IdSource, PortError, PortErrorKind,
        StoragePort, TranscriptionPort,
    },
};
use dicta_control::{
    AnnotationTool, CleanupSummary, Command as ControlCommand, Event as ControlEvent, ModelTier,
    RecordingSelector, Response,
};
use dicta_core::{
    storage::{is_shortcut_id, is_transcription_language, AppSettings},
    ProjectId, RecordingFile,
};
use dicta_engine::{
    AppState, Command as EngineCommand, CommandKind, ControllerError, Operation, StateKind,
};

impl<C, T, A, S, K, I> Runtime<C, T, A, S, K, I>
where
    C: CapturePort,
    T: TranscriptionPort,
    A: AnnotationPort,
    S: StoragePort,
    K: Clock,
    I: IdSource,
{
    pub(super) fn apply_control(
        &mut self,
        command: ControlCommand,
    ) -> Result<Response, RuntimeError> {
        self.ensure_event_capacity(12)?;
        match command {
            ControlCommand::UiShow => {
                self.publish(ControlEvent::UiShowRequested { sequence: 0 })?;
                Ok(Response::Accepted)
            }
            ControlCommand::Status | ControlCommand::RecordStatus => {
                Ok(Response::Status(self.status()))
            }
            settings_command @ (ControlCommand::SettingsGet
            | ControlCommand::SettingsSetShortcut { .. }
            | ControlCommand::SettingsSetCleanup { .. }
            | ControlCommand::SettingsSetBranchLocking { .. }
            | ControlCommand::SettingsSetLanguage { .. }
            | ControlCommand::SettingsSetGeneralPath { .. }
            | ControlCommand::SettingsCleanupMerged { .. }) => {
                self.apply_settings_control(settings_command)
            }
            ControlCommand::ModelStatus => self.model_status(),
            ControlCommand::ModelInstall { model } => self.install_model(model),
            ControlCommand::Events { follow, .. } => {
                if follow {
                    self.publish_current_state()?;
                }
                Ok(Response::Accepted)
            }
            ControlCommand::ProjectList => self.list_projects(),
            ControlCommand::ProjectAdd { path, name } => self.add_project(&path, name.as_deref()),
            ControlCommand::ProjectCreate { name } => self.create_project(&name),
            ControlCommand::ProjectRemove { project } => self.remove_project(project),
            ControlCommand::ProjectRefresh { project } => self.refresh_project(project),
            ControlCommand::ProjectSelect { project } => self.select_project(project),
            ControlCommand::ProjectCurrent => self.current_project(),
            ControlCommand::RecordingList {
                project,
                branch,
                limit,
            } => self.list_recordings(project, branch.as_deref(), limit),
            ControlCommand::RecordingShow { recording } => self.show_recording(recording),
            ControlCommand::Context {
                recording, project, ..
            } => self.recording_context(recording, project),
            ControlCommand::RecordingTranscribe { recording } => {
                self.transcribe_existing(recording)
            }
            ControlCommand::RecordingSetTimelineNotes { recording, notes } => {
                self.set_timeline_notes(recording, notes)
            }
            ControlCommand::RecordingVoiceNoteTranscribe {
                recording,
                note_id,
                timestamp_seconds,
                audio_path,
            } => self.transcribe_voice_note(recording, &note_id, timestamp_seconds, &audio_path),
            ControlCommand::RecordingVoiceNoteCancel => Ok(self.cancel_voice_note()),
            ControlCommand::RecordingVoiceNoteStatus => {
                Ok(Response::VoiceNote(self.voice_note_status.clone()))
            }
            ControlCommand::RecordingDelete { recording } => self.delete_recording(recording),
            ControlCommand::RecordStart { project, note } => self.start_recording(project, note),
            ControlCommand::RecordStop => self.stop_recording(),
            ControlCommand::RecordToggle => match self.controller.snapshot().state.kind() {
                StateKind::Idle => self.start_recording(None, None),
                StateKind::Recording | StateKind::Annotating => self.stop_recording(),
                state => Err(ControllerError::InvalidTransition {
                    command: CommandKind::StartRecording,
                    state,
                }
                .into()),
            },
            ControlCommand::AnnotationToggle => {
                if matches!(self.controller.snapshot().state, AppState::Annotating(_)) {
                    self.set_annotations_enabled(false)
                } else {
                    self.enable_pen_annotations()
                }
            }
            ControlCommand::AnnotationEnable => self.enable_pen_annotations(),
            ControlCommand::AnnotationDisable => self.set_annotations_enabled(false),
            ControlCommand::AnnotationTool { tool } => self.set_annotation_tool(tool),
            ControlCommand::AnnotationUndo => self.annotation_edit(AnnotationEdit::Undo),
            ControlCommand::AnnotationClear => self.annotation_edit(AnnotationEdit::Clear),
            ControlCommand::RecordingOpen { recording } => self.open_recording(recording),
        }
    }

    fn apply_settings_control(
        &mut self,
        command: ControlCommand,
    ) -> Result<Response, RuntimeError> {
        match command {
            ControlCommand::SettingsGet => self.settings(),
            ControlCommand::SettingsSetShortcut { shortcut_id } => {
                self.update_settings(|settings| {
                    if !is_shortcut_id(&shortcut_id) {
                        return Err(RuntimeError::InvalidRequest(format!(
                            "unknown shortcut preset `{shortcut_id}`"
                        )));
                    }
                    settings.shortcut_id = shortcut_id;
                    Ok(())
                })
            }
            ControlCommand::SettingsSetCleanup { enabled } => self.update_settings(|settings| {
                settings.cleanup_merged_videos = enabled;
                Ok(())
            }),
            ControlCommand::SettingsSetBranchLocking { enabled } => {
                self.require_idle("change branch locking")?;
                self.update_settings(|settings| {
                    settings.branch_locking = enabled;
                    Ok(())
                })
            }
            ControlCommand::SettingsSetLanguage { language } => {
                if !is_transcription_language(&language) {
                    return Err(RuntimeError::InvalidRequest(format!(
                        "unsupported transcription language `{language}`"
                    )));
                }
                self.transcription.set_language(&language)?;
                self.update_settings(|settings| {
                    settings.transcription_language = language;
                    Ok(())
                })
            }
            ControlCommand::SettingsSetGeneralPath { path } => {
                self.require_idle("change General storage")?;
                let path = validate_general_path(path)?;
                self.update_settings(|settings| {
                    settings.general_path = path;
                    Ok(())
                })
            }
            ControlCommand::SettingsCleanupMerged { project } => {
                self.require_idle("clean merged branch videos")?;
                match project {
                    Some(project) => {
                        let project_id = ProjectId::new(project)
                            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
                        self.storage
                            .cleanup_merged_videos(&project_id)
                            .map(Response::Cleanup)
                            .map_err(Into::into)
                    }
                    None => self.cleanup_all_merged_videos(),
                }
            }
            _ => Err(RuntimeError::InvalidRequest(
                "command is not a settings command".to_owned(),
            )),
        }
    }

    fn cleanup_all_merged_videos(&mut self) -> Result<Response, RuntimeError> {
        let mut summary = CleanupSummary::default();
        let mut searched = 0;
        for project in self.storage.load_projects()? {
            if project
                .source_path
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
            {
                continue;
            }
            searched += 1;
            match self.storage.cleanup_merged_videos(&project.id) {
                Ok(part) => {
                    summary.removed_files += part.removed_files;
                    summary.freed_bytes += part.freed_bytes;
                    summary.cleaned_branches.extend(part.cleaned_branches);
                    if summary.default_branch.is_none() {
                        summary.default_branch = part.default_branch;
                    }
                }
                Err(error) if error.kind == PortErrorKind::InvalidRequest => {}
                Err(error) => return Err(error.into()),
            }
        }
        summary.message = if searched == 0 {
            "No linked Git projects to clean.".to_owned()
        } else if summary.removed_files == 0 {
            "No merged videos found across linked projects.".to_owned()
        } else {
            format!(
                "Removed {} merged video{}.",
                summary.removed_files,
                if summary.removed_files == 1 { "" } else { "s" }
            )
        };
        Ok(Response::Cleanup(summary))
    }

    fn settings(&mut self) -> Result<Response, RuntimeError> {
        self.storage
            .load_settings()
            .map(settings_document)
            .map(Response::Settings)
            .map_err(Into::into)
    }

    fn update_settings(
        &mut self,
        update: impl FnOnce(&mut AppSettings) -> Result<(), RuntimeError>,
    ) -> Result<Response, RuntimeError> {
        let mut settings = self.storage.load_settings()?;
        update(&mut settings)?;
        let settings = settings.normalized();
        self.storage.save_settings(&settings)?;
        Ok(Response::Settings(settings_document(settings)))
    }

    pub(super) fn require_idle(&self, command: &'static str) -> Result<(), RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state == StateKind::Idle {
            Ok(())
        } else {
            Err(RuntimeError::CommandConflict { command, state })
        }
    }

    fn model_status(&mut self) -> Result<Response, RuntimeError> {
        self.transcription
            .model_status()
            .map(model_status_summary)
            .map(Response::ModelStatus)
            .map_err(Into::into)
    }

    fn install_model(&mut self, model: ModelTier) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle
            || self.pending_voice_note.is_some()
            || self.cancelled_voice_inflight.is_some()
        {
            return Err(RuntimeError::CommandConflict {
                command: "install a transcription model",
                state,
            });
        }
        match model {
            ModelTier::Quality => match self.transcription.install_quality_model()? {
                Completion::Ready(outcome) => {
                    Ok(Response::ModelStatus(model_status_summary(outcome.status)))
                }
                Completion::Pending => Ok(Response::ModelInstallStarted),
            },
        }
    }

    fn list_projects(&mut self) -> Result<Response, RuntimeError> {
        let selected = self.controller.snapshot().selected_project;
        let mut projects = self.storage.load_projects()?;
        projects.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(Response::Projects(
            projects
                .iter()
                .map(|project| project_summary(project, selected.as_ref()))
                .collect(),
        ))
    }

    fn add_project(&mut self, path: &str, name: Option<&str>) -> Result<Response, RuntimeError> {
        self.storage.add_project(path, name)?;
        Ok(Response::Accepted)
    }

    fn create_project(&mut self, name: &str) -> Result<Response, RuntimeError> {
        self.require_idle_project_mutation("create a project")?;
        let project = self.storage.create_project(name)?;
        self.select_project(project.id.into_string())
    }

    fn remove_project(&mut self, project: String) -> Result<Response, RuntimeError> {
        self.require_idle_project_mutation("remove a project")?;
        let project_id = ProjectId::new(project)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        if project_id.as_str() == dicta_core::GENERAL_PROJECT_ID {
            return Err(RuntimeError::InvalidRequest(
                "General cannot be removed".to_owned(),
            ));
        }
        self.storage.remove_project(&project_id)?;
        if self.controller.snapshot().selected_project.as_ref() == Some(&project_id) {
            let outcome = self
                .controller
                .dispatch(EngineCommand::SelectProject(None))?;
            self.publish_state(&outcome.snapshot)?;
        }
        Ok(Response::Accepted)
    }

    fn refresh_project(&mut self, project: String) -> Result<Response, RuntimeError> {
        let project_id = ProjectId::new(project)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let selected = self.controller.snapshot().selected_project;
        let project = self
            .storage
            .load_projects()?
            .into_iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("project {project_id} was not found"),
                )
            })?;
        Ok(Response::Project(Some(project_summary(
            &project,
            selected.as_ref(),
        ))))
    }

    fn require_idle_project_mutation(&self, command: &'static str) -> Result<(), RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state == StateKind::Idle {
            Ok(())
        } else {
            Err(RuntimeError::CommandConflict { command, state })
        }
    }

    fn select_project(&mut self, project: String) -> Result<Response, RuntimeError> {
        let project_id = ProjectId::new(project)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let projects = self.storage.load_projects()?;
        if !projects.iter().any(|project| project.id == project_id) {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                format!("project {project_id} was not found"),
            )
            .into());
        }
        let outcome = self
            .controller
            .dispatch(EngineCommand::SelectProject(Some(project_id)))?;
        self.publish_state(&outcome.snapshot)?;
        Ok(Response::Accepted)
    }

    fn current_project(&mut self) -> Result<Response, RuntimeError> {
        let Some(selected) = self.controller.snapshot().selected_project else {
            return Ok(Response::Project(None));
        };
        let project = self
            .storage
            .load_projects()?
            .into_iter()
            .find(|project| project.id == selected)
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("selected project {selected} was not found"),
                )
            })?;
        Ok(Response::Project(Some(project_summary(
            &project,
            Some(&selected),
        ))))
    }

    fn list_recordings(
        &mut self,
        project: Option<String>,
        branch: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Response, RuntimeError> {
        let project = project
            .map(|value| {
                ProjectId::new(value)
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let mut recordings = self.load_recordings_checked()?;
        recordings.retain(|recording| {
            project
                .as_ref()
                .is_none_or(|project| &recording.project_id == project)
                && branch.is_none_or(|branch| recording.git_branch.as_deref() == Some(branch))
        });
        sort_recordings_latest_first(&mut recordings);
        if let Some(limit) = limit {
            recordings.truncate(limit as usize);
        }
        Ok(Response::Recordings(
            recordings.iter().map(recording_summary).collect(),
        ))
    }

    fn show_recording(&mut self, selector: RecordingSelector) -> Result<Response, RuntimeError> {
        self.resolve_recording(selector)
            .map(recording_document)
            .map(Box::new)
            .map(Response::RecordingDetails)
    }

    fn open_recording(&mut self, selector: RecordingSelector) -> Result<Response, RuntimeError> {
        let recording = self.resolve_recording(selector)?;
        self.publish(ControlEvent::UiRecordingRequested {
            sequence: 0,
            recording_id: recording.id.into_string(),
        })?;
        Ok(Response::Accepted)
    }

    fn recording_context(
        &mut self,
        selector: RecordingSelector,
        project: Option<String>,
    ) -> Result<Response, RuntimeError> {
        let project_filter = project
            .map(|value| {
                ProjectId::new(value)
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let mut recordings = self.load_recordings_checked()?;
        if let Some(project) = project_filter.as_ref() {
            recordings.retain(|recording| &recording.project_id == project);
        }
        let recording = resolve_recording_from(recordings, selector)?;
        let project_name = self
            .storage
            .load_projects()?
            .into_iter()
            .find(|project| project.id == recording.project_id)
            .map_or_else(|| recording.project_id.to_string(), |project| project.name);
        Ok(Response::Context {
            text: render_recording_context(&recording, &project_name),
        })
    }

    fn transcribe_existing(
        &mut self,
        selector: RecordingSelector,
    ) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle
            || self.pending_voice_note.is_some()
            || self.cancelled_voice_inflight.is_some()
        {
            return Err(ControllerError::InvalidTransition {
                command: CommandKind::TranscribeRecording,
                state,
            }
            .into());
        }
        let recording = self.resolve_recording(selector)?;
        let recording_id = recording.id.clone();
        let outcome = self
            .controller
            .dispatch(EngineCommand::TranscribeRecording {
                recording_id: recording_id.clone(),
            })?;
        self.publish_state(&outcome.snapshot)?;
        self.storage.mark_transcription_pending(&recording_id)?;
        match self.transcription.transcribe(&recording) {
            Ok(Completion::Ready(output)) => {
                self.complete_transcription(recording_id, Ok(output))?;
            }
            Ok(Completion::Pending) => {}
            Err(error) => {
                self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                return Err(error.into());
            }
        }
        Ok(Response::Accepted)
    }

    fn set_timeline_notes(
        &mut self,
        selector: RecordingSelector,
        notes: Vec<dicta_control::TimelineNoteDocument>,
    ) -> Result<Response, RuntimeError> {
        if self.pending_voice_note.is_some() || self.cancelled_voice_inflight.is_some() {
            return Err(RuntimeError::InvalidRequest(
                "timeline notes cannot change while a voice note is processing".to_owned(),
            ));
        }
        let mut notes = timeline_notes_from_wire(notes);
        let recording = self.resolve_recording(selector)?;
        validate_timeline_notes(&recording, &notes)?;
        notes.sort_by(|left, right| {
            left.timestamp_seconds
                .total_cmp(&right.timestamp_seconds)
                .then_with(|| left.id.cmp(&right.id))
        });
        self.storage
            .save_timeline_notes(&recording, &notes)
            .map(recording_document)
            .map(Box::new)
            .map(Response::RecordingDetails)
            .map_err(Into::into)
    }

    pub(super) fn start_retry_transcription(
        &mut self,
        recording: &RecordingFile,
    ) -> Result<(), RuntimeError> {
        if !recording.is_valid() {
            return Err(RuntimeError::InvalidRequest(
                "retry discovery returned invalid recording metadata".to_owned(),
            ));
        }
        let recording_id = recording.id.clone();
        let outcome = self
            .controller
            .dispatch(EngineCommand::TranscribeRecording {
                recording_id: recording_id.clone(),
            })?;
        self.publish_state(&outcome.snapshot)?;
        self.storage.mark_transcription_pending(&recording_id)?;
        match self.transcription.transcribe(recording) {
            Ok(Completion::Ready(output)) => self.complete_transcription(recording_id, Ok(output)),
            Ok(Completion::Pending) => Ok(()),
            Err(error) => {
                self.raise_failure(Operation::Transcribe, recording_id, &error)?;
                Err(error.into())
            }
        }
    }

    fn delete_recording(&mut self, selector: RecordingSelector) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle {
            return Err(RuntimeError::CommandConflict {
                command: "delete a recording",
                state,
            });
        }
        let recording = self.resolve_recording(selector)?;
        self.storage.delete_recording(&recording)?;
        Ok(Response::Accepted)
    }

    pub(super) fn resolve_recording(
        &mut self,
        selector: RecordingSelector,
    ) -> Result<RecordingFile, RuntimeError> {
        resolve_recording_from(self.load_recordings_checked()?, selector)
    }

    fn load_recordings_checked(&mut self) -> Result<Vec<RecordingFile>, RuntimeError> {
        let recordings = self.storage.load_recordings()?;
        if recordings.iter().any(|recording| !recording.is_valid()) {
            return Err(PortError::new(
                PortErrorKind::Internal,
                "recording catalog returned invalid metadata",
            )
            .into());
        }
        Ok(recordings)
    }

    fn start_recording(
        &mut self,
        project: Option<String>,
        note: Option<String>,
    ) -> Result<Response, RuntimeError> {
        let state = self.controller.snapshot().state.kind();
        if state != StateKind::Idle
            || self.pending_voice_note.is_some()
            || self.cancelled_voice_inflight.is_some()
        {
            return Err(ControllerError::InvalidTransition {
                command: CommandKind::StartRecording,
                state,
            }
            .into());
        }
        let project_id = project
            .map(|value| {
                ProjectId::new(value)
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))
            })
            .transpose()?;
        let recording_id = self.ids.next_recording_id(self.clock.now())?;
        if let Some(project_id) = project_id {
            let outcome = self
                .controller
                .dispatch(EngineCommand::SelectProject(Some(project_id)))?;
            self.publish_state(&outcome.snapshot)?;
        }
        let outcome = self.controller.dispatch(EngineCommand::StartRecording {
            recording_id: recording_id.clone(),
            note,
        })?;
        self.publish_state(&outcome.snapshot)?;
        let session = session_from_state(&outcome.snapshot.state).clone();
        match self.capture.start(&session) {
            Ok(Completion::Ready(())) => self.complete_capture_start_inner(recording_id)?,
            Ok(Completion::Pending) => {}
            Err(error) => {
                self.raise_failure(Operation::PrepareRecording, recording_id, &error)?;
                return Err(error.into());
            }
        }
        Ok(Response::Accepted)
    }

    fn stop_recording(&mut self) -> Result<Response, RuntimeError> {
        let outcome = self.controller.dispatch(EngineCommand::StopRecording)?;
        self.publish_state(&outcome.snapshot)?;
        let session = session_from_state(&outcome.snapshot.state).clone();
        let recording_id = session.recording_id.clone();
        match self.capture.stop(&session) {
            Ok(Completion::Ready(artifact)) => {
                self.complete_capture_stop_inner(&session, &artifact)?;
            }
            Ok(Completion::Pending) => {}
            Err(error) => {
                self.raise_failure(Operation::StopRecording, recording_id, &error)?;
                return Err(error.into());
            }
        }
        Ok(Response::Accepted)
    }

    fn set_annotations_enabled(&mut self, enabled: bool) -> Result<Response, RuntimeError> {
        let command = if enabled {
            EngineCommand::StartAnnotating
        } else {
            EngineCommand::StopAnnotating
        };
        let outcome = self.controller.dispatch(command)?;
        let session = session_from_state(&outcome.snapshot.state).clone();
        if let Err(error) = self.annotations.set_enabled(&session.recording_id, enabled) {
            self.raise_failure(Operation::Capture, session.recording_id, &error)?;
            return Err(error.into());
        }
        self.publish_state(&outcome.snapshot)?;
        Ok(Response::Accepted)
    }

    fn enable_pen_annotations(&mut self) -> Result<Response, RuntimeError> {
        self.set_annotations_enabled(true)?;
        self.set_annotation_tool(AnnotationTool::Pen)
    }

    fn set_annotation_tool(&mut self, tool: AnnotationTool) -> Result<Response, RuntimeError> {
        let session = self.require_annotating_session()?;
        if let Err(error) = self.annotations.set_tool(&session.recording_id, tool) {
            self.raise_failure(Operation::Capture, session.recording_id, &error)?;
            return Err(error.into());
        }
        self.selected_tool = tool;
        self.publish_current_state()?;
        Ok(Response::Accepted)
    }

    fn annotation_edit(&mut self, edit: AnnotationEdit) -> Result<Response, RuntimeError> {
        let session = self.require_annotating_session()?;
        let result = match edit {
            AnnotationEdit::Undo => self.annotations.undo(&session.recording_id),
            AnnotationEdit::Clear => self.annotations.clear(&session.recording_id),
        };
        if let Err(error) = result {
            self.raise_failure(Operation::Capture, session.recording_id, &error)?;
            return Err(error.into());
        }
        Ok(Response::Accepted)
    }
}
