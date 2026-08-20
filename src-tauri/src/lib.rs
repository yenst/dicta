use chrono::{DateTime, Local, Utc};
use dicta_core::transcript::format_timestamp as transcript_timestamp;
use dicta_core::{
    branch as core_branch, git as core_git, BranchMetadata, ProjectFile, ProjectId, RecordingId,
    RecordingScope, TranscriptSegment, TranscriptionStatus,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    ffi::CStr,
    fs::{self, OpenOptions},
    hash::{Hash, Hasher},
    io::Read,
    os::raw::c_char,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

mod app;
mod integrations;
mod platform;
mod recorder;
mod storage;
mod transcription;
use app::commands::*;
use app::context::*;
use app::state::*;
use app::tray::*;
use integrations::git::*;
#[cfg(test)]
use integrations::mcp::{atomic_install_binary, register_codex_mcp};
use integrations::mcp::{
    configure_codex_mcp, install_binary as install_mcp_binary, mcp_status, restart_codex_mcp,
};
use recorder::*;
use storage::projects::*;
#[cfg(test)]
use storage::settings::normalize as normalize_settings;
use storage::settings::{
    is_allowed_language, language as settings_language, read as read_settings, shortcut_for_id,
    write as write_settings, AppSettings,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use transcription::*;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static LOCAL_TRANSCRIBER: OnceLock<Mutex<()>> = OnceLock::new();
static WHISPER_MODEL: OnceLock<Mutex<Option<LoadedWhisper>>> = OnceLock::new();
static UNIQUE_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

const QUALITY_MODEL_FILENAME: &str = "ggml-large-v3-turbo-q5_0.bin";
const MAX_RECORDING_SECONDS: u64 = 20 * 60;
const TRAY_ID: &str = "dicta";
const ALLOWED_LANGUAGES: [&str; 6] = ["auto", "nl", "en", "fr", "de", "es"];
const DEFAULT_LANGUAGE: &str = "auto";
const UNPROJECTED_ID: &str = dicta_core::GENERAL_PROJECT_ID;
const QUALITY_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";
const QUALITY_MODEL_SHA1: &str = "e050f7970618a659205450ad97eb95a18d69c9ee";
const QUALITY_MODEL_DOWNLOAD_BYTES: u64 = 547 * 1024 * 1024;
#[cfg(target_os = "linux")]
const DEFAULT_SHORTCUT_ID: &str = "alt_shift_r";
#[cfg(not(target_os = "linux"))]
const DEFAULT_SHORTCUT_ID: &str = "command_shift_r";

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let documents = dirs::document_dir().unwrap_or_else(|| PathBuf::from("."));
    let legacy_root = documents.join("PromptReel");
    let preferred_root = documents.join("Dicta");
    let root = if preferred_root.exists() {
        preferred_root
    } else if legacy_root.exists() {
        match fs::rename(&legacy_root, &preferred_root) {
            Ok(()) => preferred_root,
            Err(_) => legacy_root,
        }
    } else {
        preferred_root
    };
    fs::create_dir_all(&root).expect("failed to create Dicta folder");
    let old_general = root.join("unprojected");
    let general = root.join("General");
    if old_general.is_dir() && !general.exists() {
        let _ = fs::rename(&old_general, &general);
    }

    let settings = read_settings(&root);
    let shortcut = shortcut_for_id(&settings.shortcut_id)
        .unwrap_or_else(|| shortcut_for_id(DEFAULT_SHORTCUT_ID).expect("default shortcut exists"));

    let app = tauri::Builder::default()
        .manage(AppState::new(root))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _triggered, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_from_shortcut(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            mcp_status,
            set_general_path,
            configure_codex_mcp,
            restart_codex_mcp,
            model_status,
            download_quality_model,
            create_project,
            link_project,
            remove_project,
            refresh_project,
            get_app_settings,
            set_shortcut,
            set_cleanup_merged_videos,
            set_branch_locking,
            set_transcription_language,
            cleanup_merged_videos,
            ensure_recording_poster,
            select_project,
            list_recordings,
            delete_recording,
            save_timeline_notes,
            transcribe_voice_note,
            retranscribe_recording,
            start_recording,
            stop_recording,
            reveal_path,
            build_context,
            build_recording_context,
            copy_to_clipboard
        ])
        .setup(move |app| {
            let _ = APP_HANDLE.set(app.handle().clone());
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("main") {
                window.set_decorations(false)?;
            }
            let _ = install_mcp_binary(app.handle());
            ensure_default_project_selection(app.handle());
            #[cfg(target_os = "linux")]
            if let Err(error) =
                platform::linux::control::start(app.handle().clone(), toggle_from_shortcut)
            {
                eprintln!("{error}");
            }
            let root = app.state::<AppState>().root.clone();
            queue_pending_transcriptions(&root);
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            if let Err(error) = app.global_shortcut().register(shortcut) {
                eprintln!("Dicta could not register its global shortcut: {error}");
            }

            let menu = build_tray_menu(app.handle())?;
            let mut tray = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Dicta")
                .on_menu_event(|app, event| handle_tray_menu_event(app, event.id.as_ref()))
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                });
            #[cfg(target_os = "macos")]
            {
                if let Ok(icon) =
                    tauri::image::Image::from_bytes(include_bytes!("../assets/dicta-tray@2x.png"))
                {
                    tray = tray.icon(icon).icon_as_template(true);
                } else if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!(
                    "../../src/assets/dicta-mark-light.png"
                )) {
                    tray = tray.icon(icon);
                } else if let Some(icon) = app.default_window_icon() {
                    tray = tray.icon(icon.clone());
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Dicta");
    app.run(|_, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            platform::abort_recording();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn project_id(value: &str) -> ProjectId {
        ProjectId::new(value).unwrap()
    }

    fn recording_id(value: &str) -> RecordingId {
        RecordingId::new(value).unwrap()
    }

    fn test_inner(phase: RecordingPhase, with_session: bool) -> InnerState {
        let recording = with_session.then(|| Recording {
            id: recording_id("test-recording"),
            project_id: project_id("test-project"),
            video_path: "/tmp/test-recording.mp4".into(),
            metadata_path: "/tmp/test-recording.json".into(),
            note: String::new(),
            recording_scope: RecordingScope::Repository,
            git_branch: None,
            started_at: Utc::now(),
            ended_at: None,
            duration_seconds: None,
            size_bytes: None,
            success: false,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: TranscriptionStatus::Pending,
            transcription_error: None,
            transcription_language: None,
            poster_path: None,
            timeline_notes: Vec::new(),
        });
        InnerState {
            status: RecorderStatus {
                phase,
                active_project_id: Some("test-project".into()),
                active_video_path: recording
                    .as_ref()
                    .map(|recording| recording.video_path.clone()),
                started_at: None,
                last_error: None,
            },
            session: recording,
            last_note: String::new(),
        }
    }

    #[test]
    fn creates_safe_project_slugs() {
        assert_eq!(
            slugify(" API Integration / Tickets "),
            "api-integration-tickets"
        );
        assert_eq!(slugify("✨"), "project");
    }

    #[test]
    fn project_and_recording_reservations_never_reuse_paths() {
        let root = std::env::temp_dir().join(format!(
            "dicta-reservation-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let created_at = Utc::now();
        let (first_id, first_project) =
            reserve_project_directory(&root, "Demo", &created_at).unwrap();
        let (second_id, second_project) =
            reserve_project_directory(&root, "Demo", &created_at).unwrap();
        assert_ne!(first_id, second_id);
        assert_ne!(first_project, second_project);
        assert!(first_project.is_dir());
        assert!(second_project.is_dir());

        let day = root.join("recordings/2026-08-20");
        fs::create_dir_all(&day).unwrap();
        let started_at = Local::now();
        let (first_stem, first_video, first_metadata) =
            reserve_recording_paths(&day, &started_at).unwrap();
        let (second_stem, second_video, second_metadata) =
            reserve_recording_paths(&day, &started_at).unwrap();
        assert_ne!(first_stem, second_stem);
        assert_ne!(first_video, second_video);
        assert_ne!(first_metadata, second_metadata);
        assert!(first_metadata.is_file());
        assert!(second_metadata.is_file());
        assert!(!first_video.exists());
        assert!(!second_video.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_events_require_a_compatible_live_session() {
        let preparing = test_inner(RecordingPhase::Preparing, true);
        assert!(accepts_capture_event(&preparing, "started"));
        assert!(accepts_capture_event(&preparing, "error"));
        assert!(!accepts_capture_event(&preparing, "finished"));

        let recording = test_inner(RecordingPhase::Recording, true);
        assert!(!accepts_capture_event(&recording, "started"));
        assert!(accepts_capture_event(&recording, "finished"));

        let without_session = test_inner(RecordingPhase::Preparing, false);
        assert!(!accepts_capture_event(&without_session, "started"));

        let mut stopping = test_inner(RecordingPhase::Stopping, true);
        restore_after_stop_rejection(&mut stopping, "stop rejected");
        assert!(matches!(stopping.status.phase, RecordingPhase::Recording));
        assert_eq!(stopping.status.last_error.as_deref(), Some("stop rejected"));
    }

    #[test]
    fn removing_a_project_preserves_its_files_and_archives_registration() {
        let root = std::env::temp_dir().join(format!(
            "dicta-remove-project-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let project_path = root.join("demo");
        let recording_path = project_path.join("recordings/2026-08-14/demo.mp4");
        fs::create_dir_all(recording_path.parent().unwrap()).unwrap();
        fs::write(&recording_path, "video").unwrap();
        let metadata = ProjectFile {
            id: project_id("demo"),
            name: "Demo".to_string(),
            created_at: Utc::now(),
            source_path: None,
        };
        fs::write(
            project_path.join("project.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .unwrap();

        remove_project_registration(&root, "demo").unwrap();

        assert!(!project_path.join("project.json").exists());
        assert!(recording_path.is_file());
        assert!(fs::read_dir(&project_path)
            .unwrap()
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("project.removed-")));
        assert_eq!(load_projects(&root).len(), 1);
        assert_eq!(load_projects(&root)[0].id, UNPROJECTED_ID);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mcp_binary_install_replaces_the_file_atomically() {
        let root = std::env::temp_dir().join(format!(
            "dicta-mcp-install-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let resource = root.join("resource/dicta-mcp");
        let target = root.join("installed/dicta-mcp");
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&resource, "new executable").unwrap();
        fs::write(&target, "running executable").unwrap();
        let open_old_file = fs::File::open(&target).unwrap();

        atomic_install_binary(&resource, &target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new executable");
        let mut old_contents = String::new();
        use std::io::Read;
        (&open_old_file).read_to_string(&mut old_contents).unwrap();
        assert_eq!(old_contents, "running executable");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn forced_mcp_registration_removes_then_adds_the_server() {
        let root = std::env::temp_dir().join(format!(
            "dicta-mcp-restart-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let fake_codex = root.join("codex");
        let executable = root.join("dicta-mcp");
        let log = root.join("calls.log");
        fs::write(&executable, "server").unwrap();
        fs::write(
            &fake_codex,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                path_string(&log)
            ),
        )
        .unwrap();
        fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();

        register_codex_mcp(&fake_codex, &executable, true).unwrap();

        let calls = fs::read_to_string(log).unwrap();
        let lines = calls.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "mcp remove dicta");
        assert_eq!(
            lines[1],
            format!("mcp add dicta -- {}", path_string(&executable))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_file_scan_ignores_non_metadata_files() {
        let root = std::env::temp_dir().join(format!(
            "dicta-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let day = root.join("recordings/2026-08-13");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("test.json"), "{}").unwrap();
        fs::write(day.join("test.mp4"), "video").unwrap();
        assert_eq!(recording_files(&root), vec![day.join("test.json")]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_artifacts_stay_beside_the_metadata() {
        let metadata = PathBuf::from("/tmp/dicta/14-25-09.json");
        let artifacts = recording_artifact_paths(&metadata);
        assert_eq!(artifacts.len(), 7);
        assert!(artifacts.contains(&PathBuf::from("/tmp/dicta/14-25-09.mp4")));
        assert!(artifacts.contains(&PathBuf::from("/tmp/dicta/14-25-09.poster.jpg")));
        assert!(artifacts.contains(&PathBuf::from("/tmp/dicta/14-25-09.transcript.base.md")));
        assert!(artifacts
            .iter()
            .all(|path| path.parent() == metadata.parent()));
    }

    #[test]
    fn permission_denied_start_discards_pending_recording_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "dicta-permission-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let day = root.join("recordings/2026-08-13");
        fs::create_dir_all(&day).unwrap();
        let video_path = day.join("20-48-00.mp4");
        let metadata_path = day.join("20-48-00.json");
        fs::write(&video_path, []).unwrap();
        fs::write(&metadata_path, "pending").unwrap();
        let recording = Recording {
            id: recording_id("20260813-20-48-00"),
            project_id: project_id("peepel"),
            video_path: path_string(&video_path),
            metadata_path: path_string(&metadata_path),
            note: String::new(),
            recording_scope: RecordingScope::Branch,
            git_branch: Some("securex-quota".to_string()),
            started_at: Utc::now(),
            ended_at: None,
            duration_seconds: None,
            size_bytes: None,
            success: false,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: TranscriptionStatus::Pending,
            transcription_error: None,
            transcription_language: None,
            poster_path: None,
            timeline_notes: Vec::new(),
        };

        discard_recording_artifacts(&recording);

        assert!(!video_path.exists());
        assert!(!metadata_path.exists());
        assert!(!day.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_branch_packets_are_migrated_without_losing_files() {
        let root = std::env::temp_dir().join(format!(
            "dicta-branch-migration-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("repository");
        let branch = "feature/oauth";
        let legacy = repository
            .join(".dicta/branches")
            .join(core_branch::legacy_folder_name(branch));
        fs::create_dir_all(legacy.join("recordings")).unwrap();
        fs::write(
            legacy.join("branch.json"),
            serde_json::json!({ "git_branch": branch }).to_string(),
        )
        .unwrap();
        fs::write(legacy.join("recordings/keep.mp4"), "video").unwrap();
        fs::write(legacy.join("recordings/keep.poster.jpg"), "poster").unwrap();
        fs::write(legacy.join("recordings/keep.transcript.md"), "words").unwrap();
        let legacy_metadata = legacy.join("recordings/keep.json");
        fs::write(
            &legacy_metadata,
            serde_json::json!({
                "video_path": path_string(&legacy.join("recordings/keep.mp4")),
                "metadata_path": path_string(&legacy_metadata),
                "poster_path": path_string(&legacy.join("recordings/keep.poster.jpg")),
                "transcript_path": path_string(&legacy.join("recordings/keep.transcript.md"))
            })
            .to_string(),
        )
        .unwrap();
        let project = ProjectFile {
            id: project_id("repository"),
            name: "Repository".into(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };

        let resolved = resolved_branch_dir(&project, branch).unwrap();

        assert_eq!(
            resolved,
            repository
                .join(".dicta/branches")
                .join(core_branch::folder_name(branch))
        );
        assert!(resolved.join("recordings/keep.mp4").is_file());
        assert!(!legacy.exists());
        let migrated: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(resolved.join("recordings/keep.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            migrated["poster_path"].as_str(),
            Some(path_string(&resolved.join("recordings/keep.poster.jpg")).as_str())
        );
        assert_eq!(
            migrated["transcript_path"].as_str(),
            Some(path_string(&resolved.join("recordings/keep.transcript.md")).as_str())
        );

        fs::create_dir_all(legacy.join("recordings")).unwrap();
        fs::write(
            legacy.join("branch.json"),
            serde_json::json!({ "git_branch": branch }).to_string(),
        )
        .unwrap();
        fs::write(legacy.join("recordings/late-copy.mp4"), "video").unwrap();
        let resolved_again = resolved_branch_dir(&project, branch).unwrap();
        assert_eq!(resolved_again, resolved);
        assert!(resolved.join("recordings/late-copy.mp4").is_file());
        assert!(!legacy.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_round_trip_shortcut_and_cleanup_preferences() {
        let root = std::env::temp_dir().join(format!(
            "dicta-settings-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(read_settings(&root).shortcut_id, DEFAULT_SHORTCUT_ID);
        assert!(read_settings(&root).cleanup_merged_videos);
        assert_eq!(
            read_settings(&root).transcription_language,
            DEFAULT_LANGUAGE
        );

        let settings = AppSettings {
            shortcut_id: "option_space".to_string(),
            cleanup_merged_videos: false,
            branch_locking: false,
            transcription_language: "en".to_string(),
            general_path: None,
        };
        write_settings(&root, &settings).unwrap();
        let restored = read_settings(&root);
        assert_eq!(restored.shortcut_id, "option_space");
        assert!(!restored.cleanup_merged_videos);
        assert!(!restored.branch_locking);
        assert_eq!(restored.transcription_language, "en");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn migrates_the_macos_default_shortcut_on_linux() {
        let settings = normalize_settings(AppSettings {
            shortcut_id: "command_shift_r".to_string(),
            cleanup_merged_videos: true,
            branch_locking: true,
            transcription_language: "auto".to_string(),
            general_path: None,
        });
        assert_eq!(settings.shortcut_id, "alt_shift_r");
    }

    #[test]
    fn merged_branch_cleanup_only_removes_videos() {
        let root = std::env::temp_dir().join(format!(
            "dicta-cleanup-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("repository");
        let library = root.join("library");
        fs::create_dir_all(&library).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        for arguments in [
            ["config", "user.email", "dicta@example.com"],
            ["config", "user.name", "Dicta Tests"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(arguments)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repository.join("README.md"), "main").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["commit", "-m", "main"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "-b", "feature/context"])
            .status()
            .unwrap()
            .success());
        fs::write(repository.join("feature.txt"), "feature").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["add", "feature.txt"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["commit", "-m", "feature"])
            .status()
            .unwrap()
            .success());

        let branch_root = repository.join(".dicta/branches/feature__context");
        write_branch_metadata(&branch_root, "feature/context", Some(&repository)).unwrap();
        let day = branch_root.join("recordings/2026-08-13");
        fs::create_dir_all(&day).unwrap();
        fs::write(day.join("packet.mp4"), vec![7_u8; 128]).unwrap();
        fs::write(day.join("packet.transcript.md"), "keep me").unwrap();
        fs::write(day.join("packet.json"), "{}").unwrap();

        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "main"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["merge", "--no-ff", "feature/context", "-m", "merge feature"])
            .status()
            .unwrap()
            .success());

        let project = ProjectFile {
            id: project_id("repository"),
            name: "Repository".to_string(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };
        let summary = cleanup_merged_videos_for_project(&library, &project).unwrap();
        assert_eq!(summary.removed_files, 1);
        assert_eq!(summary.freed_bytes, 128);
        assert_eq!(summary.cleaned_branches, vec!["feature/context"]);
        assert!(!day.join("packet.mp4").exists());
        assert!(day.join("packet.transcript.md").is_file());
        assert!(day.join("packet.json").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_root_follows_the_checked_out_branch() {
        let root = std::env::temp_dir().join(format!(
            "dicta-git-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("peepel");
        let storage = root.join("storage");
        fs::create_dir_all(&storage).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        let project = ProjectFile {
            id: project_id("peepel"),
            name: "peepel".to_string(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };

        let (branch, path) = active_recording_root(&storage, &project).unwrap();
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(path, repository.join(".dicta/branches/v2-main"));

        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["checkout", "-b", "feature/oauth"])
            .status()
            .unwrap()
            .success());
        let (branch, path) = active_recording_root(&storage, &project).unwrap();
        assert_eq!(branch.as_deref(), Some("feature/oauth"));
        assert_eq!(path, repository.join(".dicta/branches/v2-feature%2Foauth"));

        write_settings(
            &storage,
            &AppSettings {
                branch_locking: false,
                ..AppSettings::default()
            },
        )
        .unwrap();
        let (branch, path) = active_recording_root(&storage, &project).unwrap();
        assert_eq!(branch, None);
        assert_eq!(path, repository.join(".dicta"));

        let (branch, path) = active_recording_root(&storage, &unprojected_metadata()).unwrap();
        assert_eq!(branch, None);
        assert_eq!(path, storage.join("General"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_storage_migrates_packets_and_rewrites_paths() {
        let root = std::env::temp_dir().join(format!(
            "dicta-migration-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let repository = root.join("securex");
        let library = root.join("library");
        assert!(std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&repository)
            .status()
            .unwrap()
            .success());
        let metadata = ProjectFile {
            id: project_id("securex"),
            name: "securex".to_string(),
            created_at: Utc::now(),
            source_path: Some(path_string(&repository)),
        };
        let legacy = library.join("securex");
        let old_day = legacy.join("branches/main/recordings/2026-08-13");
        fs::create_dir_all(&old_day).unwrap();
        let old_video = old_day.join("securex-quotas.mp4");
        let old_metadata = old_day.join("securex-quotas.json");
        fs::write(&old_video, "video").unwrap();
        fs::write(
            &old_metadata,
            serde_json::json!({
                "id": "securex-quotas",
                "video_path": path_string(&old_video),
                "metadata_path": path_string(&old_metadata)
            })
            .to_string(),
        )
        .unwrap();

        let destination = prepare_linked_storage(&library, &metadata).unwrap();
        let migrated_metadata =
            destination.join("branches/main/recordings/2026-08-13/securex-quotas.json");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(migrated_metadata).unwrap()).unwrap();
        assert_eq!(
            value["video_path"].as_str().unwrap(),
            path_string(
                &destination.join("branches/main/recordings/2026-08-13/securex-quotas.mp4")
            )
        );
        assert!(destination.join("project.json").is_file());
        let raw_exclude = PathBuf::from(
            git_output(&repository, &["rev-parse", "--git-path", "info/exclude"]).unwrap(),
        );
        let exclude_path = if raw_exclude.is_absolute() {
            raw_exclude
        } else {
            repository.join(raw_exclude)
        };
        assert!(fs::read_to_string(exclude_path)
            .unwrap()
            .contains(".dicta/"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn whisper_prompts_stay_generic() {
        assert!(!whisper_prompt("nl").to_lowercase().contains("securex"));
        assert!(!whisper_prompt("en").to_lowercase().contains("securex"));
        assert!(whisper_prompt("nl").contains("broncode"));
    }

    #[test]
    fn transcript_segments_are_grouped_and_timestamped() {
        let segments = normalize_transcript_segments(&[
            TranscriptSegment {
                start_seconds: 1.0,
                end_seconds: 1.4,
                text: "Open".into(),
            },
            TranscriptSegment {
                start_seconds: 1.5,
                end_seconds: 2.0,
                text: "the retry menu.".into(),
            },
            TranscriptSegment {
                start_seconds: 7.0,
                end_seconds: 9.25,
                text: "Inspect the response.".into(),
            },
        ]);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Open the retry menu.");
        assert_eq!(
            timestamped_transcript("fallback", &segments),
            "[00:01.0–00:02.0] Open the retry menu.\n[00:07.0–00:09.3] Inspect the response."
        );
    }

    #[test]
    fn update_transcription_persists_timed_sidecars() {
        let root = std::env::temp_dir().join(format!(
            "dicta-timed-transcript-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&root).unwrap();
        let video_path = root.join("recording.mp4");
        let metadata_path = root.join("recording.json");
        fs::write(&video_path, []).unwrap();
        let recording = Recording {
            id: recording_id("timed"),
            project_id: project_id("demo"),
            video_path: path_string(&video_path),
            metadata_path: path_string(&metadata_path),
            note: String::new(),
            recording_scope: RecordingScope::Branch,
            git_branch: Some("main".into()),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            duration_seconds: Some(30.0),
            size_bytes: Some(0),
            success: true,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: TranscriptionStatus::Processing,
            transcription_error: None,
            transcription_language: Some("en".into()),
            poster_path: None,
            timeline_notes: Vec::new(),
        };
        write_recording(&recording).unwrap();
        let updated = update_transcription(&NativeTranscriptionPayload {
            path: path_string(&video_path),
            transcript: Some("Open the retry menu".into()),
            transcript_segments: vec![TranscriptSegment {
                start_seconds: 12.0,
                end_seconds: 15.5,
                text: "Open the retry menu".into(),
            }],
            error: None,
        })
        .unwrap();
        assert_eq!(updated.transcript_segments.len(), 1);
        assert!(fs::read_to_string(root.join("recording.transcript.md"))
            .unwrap()
            .contains("[00:12.0–00:15.5]"));
        assert!(fs::read_to_string(root.join("recording.transcript.json"))
            .unwrap()
            .contains("transcript_segments"));
        let stored: Recording =
            serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
        assert_eq!(stored.transcript_segments[0].start_seconds, 12.0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_transcripts_are_not_retried() {
        let recording = Recording {
            id: recording_id("one"),
            project_id: project_id("demo"),
            video_path: "/missing.mp4".into(),
            metadata_path: "/missing.json".into(),
            note: String::new(),
            recording_scope: RecordingScope::Repository,
            git_branch: None,
            started_at: Utc::now(),
            ended_at: None,
            duration_seconds: None,
            size_bytes: None,
            success: true,
            transcript: None,
            transcript_path: None,
            transcript_segments: Vec::new(),
            transcription_status: TranscriptionStatus::Failed,
            transcription_error: Some("no speech".into()),
            transcription_language: Some("en".into()),
            poster_path: None,
            timeline_notes: Vec::new(),
        };
        assert!(!should_retry_transcription(&recording));
        let mut pending = recording.clone();
        pending.transcription_status = TranscriptionStatus::Pending;
        pending.video_path = path_string(&std::env::temp_dir().join("dicta-retry-missing.mp4"));
        assert!(!should_retry_transcription(&pending));
    }

    #[test]
    fn invalid_settings_language_falls_back_to_auto() {
        let settings = normalize_settings(AppSettings {
            shortcut_id: "command_shift_r".into(),
            cleanup_merged_videos: true,
            branch_locking: true,
            transcription_language: "xx".into(),
            general_path: None,
        });
        assert_eq!(settings.transcription_language, DEFAULT_LANGUAGE);
    }
}
