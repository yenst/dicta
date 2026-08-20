use crate::*;

pub(crate) fn ensure_default_project_selection(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut inner = state
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if inner.status.active_project_id.is_some() {
        return;
    }
    let projects = load_projects(&state.root);
    inner.status.active_project_id = projects
        .iter()
        .find(|project| project.id != UNPROJECTED_ID)
        .or_else(|| projects.first())
        .map(|project| project.id.clone());
}

pub(crate) fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let inner = state
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let active_project_id = inner.status.active_project_id.clone();
    let recording = matches!(
        inner.status.phase,
        RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
    );
    let record_label = if matches!(inner.status.phase, RecordingPhase::Recording) {
        "Stop Recording"
    } else {
        "Start Recording"
    };
    drop(inner);

    let projects = load_projects(&state.root);
    let project_menu = Submenu::with_id(app, "projects", "Projects", !projects.is_empty())?;
    for project in projects {
        let item = CheckMenuItem::with_id(
            app,
            format!("project:{}", project.id),
            project.name,
            !recording,
            active_project_id.as_deref() == Some(project.id.as_str()),
            None::<&str>,
        )?;
        project_menu.append(&item)?;
    }

    let show = MenuItem::with_id(app, "show", "Show Dicta", true, None::<&str>)?;
    let record = MenuItem::with_id(app, "record", record_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Dicta", true, None::<&str>)?;
    Menu::with_items(app, &[&show, &project_menu, &record, &quit])
}

pub(crate) fn sync_tray_menu(app: &AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_tray_menu(app)?))?;
    }
    Ok(())
}

pub(crate) fn select_project_from_tray(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _ = read_project(&state.root, project_id)?;
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| "Recorder state is unavailable".to_string())?;
    if matches!(
        inner.status.phase,
        RecordingPhase::Preparing | RecordingPhase::Recording | RecordingPhase::Stopping
    ) {
        return Err("Cannot change projects while recording".to_string());
    }
    inner.status.active_project_id = Some(project_id.to_string());
    drop(inner);
    let _ = sync_tray_menu(app);
    let _ = app.emit("project-selected", project_id.to_string());
    Ok(())
}

pub(crate) fn handle_tray_menu_event(app: &AppHandle, id: &str) {
    match id {
        "show" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "record" => toggle_from_shortcut(app),
        "quit" => {
            platform::abort_recording();
            app.exit(0);
        }
        _ => {
            if let Some(project_id) = id.strip_prefix("project:") {
                if let Err(error) = select_project_from_tray(app, project_id) {
                    let state = app.state::<AppState>();
                    let status = {
                        let mut inner = state
                            .inner
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        inner.status.last_error = Some(error.clone());
                        inner.status.clone()
                    };
                    emit_recorder_event(app, "error", &error, status);
                }
            }
        }
    }
}
