use dicta_control::{
    cli::{CliInvocation, OutputFormat},
    AnnotationTool, Command, ModelTier, RecordingSelector,
};

#[test]
fn parses_machine_readable_recording_and_annotation_commands() {
    assert_eq!(
        CliInvocation::parse([
            "context",
            "latest",
            "--project",
            "dicta",
            "--copy",
            "--json"
        ])
        .unwrap(),
        CliInvocation {
            command: Command::Context {
                recording: RecordingSelector::Latest,
                project: Some("dicta".to_string()),
                copy: true,
            },
            output: OutputFormat::Json,
        }
    );
    assert_eq!(
        CliInvocation::parse(["annotate", "tool", "spotlight"])
            .unwrap()
            .command,
        Command::AnnotationTool {
            tool: AnnotationTool::Spotlight,
        }
    );
}

#[test]
fn rejects_unknown_commands_and_tools() {
    assert!(CliInvocation::parse(["omarchy", "state"]).is_err());
    assert!(CliInvocation::parse(["annotate", "tool", "laser"]).is_err());
}

#[test]
fn parses_recording_filters_in_any_order() {
    assert_eq!(
        CliInvocation::parse([
            "recording",
            "list",
            "--limit",
            "3",
            "--branch",
            "main",
            "--project",
            "dicta"
        ])
        .unwrap()
        .command,
        Command::RecordingList {
            project: Some("dicta".to_string()),
            branch: Some("main".to_string()),
            limit: Some(3),
        }
    );
}

#[test]
fn parses_project_create_link_refresh_and_remove_commands() {
    assert!(matches!(
        CliInvocation::parse(["project", "add", "/repo", "--name", "Repo"])
            .unwrap()
            .command,
        Command::ProjectAdd { name: Some(_), .. }
    ));
    assert_eq!(
        CliInvocation::parse(["project", "create", "Scratch"])
            .unwrap()
            .command,
        Command::ProjectCreate {
            name: "Scratch".to_owned(),
        }
    );
    assert_eq!(
        CliInvocation::parse(["project", "refresh", "repo"])
            .unwrap()
            .command,
        Command::ProjectRefresh {
            project: "repo".to_owned(),
        }
    );
    assert_eq!(
        CliInvocation::parse(["project", "remove", "repo"])
            .unwrap()
            .command,
        Command::ProjectRemove {
            project: "repo".to_owned(),
        }
    );
}

#[test]
fn parses_ui_show_command() {
    assert_eq!(
        CliInvocation::parse(["ui"]).unwrap().command,
        Command::UiShow
    );
}

#[test]
fn parses_typed_model_status_and_quality_install_commands() {
    assert_eq!(
        CliInvocation::parse(["model", "status"]).unwrap().command,
        Command::ModelStatus
    );
    assert_eq!(
        CliInvocation::parse(["model", "install", "quality", "--json"]).unwrap(),
        CliInvocation {
            command: Command::ModelInstall {
                model: ModelTier::Quality,
            },
            output: OutputFormat::Json,
        }
    );
    assert!(CliInvocation::parse(["model", "install", "compact"]).is_err());
}

#[test]
fn parses_typed_settings_commands_and_rejects_invalid_switches() {
    assert_eq!(
        CliInvocation::parse(["settings", "get"]).unwrap().command,
        Command::SettingsGet
    );
    assert_eq!(
        CliInvocation::parse(["settings", "language", "nl"])
            .unwrap()
            .command,
        Command::SettingsSetLanguage {
            language: "nl".to_owned(),
        }
    );
    assert_eq!(
        CliInvocation::parse(["settings", "cleanup", "off"])
            .unwrap()
            .command,
        Command::SettingsSetCleanup { enabled: false }
    );
    assert_eq!(
        CliInvocation::parse(["settings", "general-path", "default"])
            .unwrap()
            .command,
        Command::SettingsSetGeneralPath { path: None }
    );
    assert_eq!(
        CliInvocation::parse(["settings", "cleanup-now", "--project", "dicta"])
            .unwrap()
            .command,
        Command::SettingsCleanupMerged {
            project: Some("dicta".to_owned()),
        }
    );
    assert!(CliInvocation::parse(["settings", "branch-locking", "maybe"]).is_err());
}
