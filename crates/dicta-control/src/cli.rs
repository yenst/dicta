use crate::{AnnotationTool, Command, ModelTier, RecordingSelector};
use std::{error::Error, fmt, path::PathBuf, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CliInvocation {
    pub command: Command,
    pub output: OutputFormat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliError(pub String);

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CliError {}

impl CliInvocation {
    pub fn parse<I, S>(arguments: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
        let output = if let Some(position) = args.iter().position(|arg| arg == "--json") {
            args.remove(position);
            OutputFormat::Json
        } else {
            OutputFormat::Human
        };
        let command = parse_command(&args)?;
        Ok(Self { command, output })
    }
}

fn parse_command(args: &[String]) -> Result<Command, CliError> {
    let words = args.iter().map(String::as_str).collect::<Vec<_>>();
    if words.starts_with(&["recording", "list"]) {
        return parse_recording_list(&words[2..]);
    }
    match words.as_slice() {
        ["status"] => Ok(Command::Status),
        ["settings", "get"] => Ok(Command::SettingsGet),
        ["settings", "shortcut", shortcut_id] => Ok(Command::SettingsSetShortcut {
            shortcut_id: (*shortcut_id).to_string(),
        }),
        ["settings", "cleanup", enabled] => Ok(Command::SettingsSetCleanup {
            enabled: parse_switch("cleanup", enabled)?,
        }),
        ["settings", "branch-locking", enabled] => Ok(Command::SettingsSetBranchLocking {
            enabled: parse_switch("branch-locking", enabled)?,
        }),
        ["settings", "language", language] => Ok(Command::SettingsSetLanguage {
            language: (*language).to_string(),
        }),
        ["settings", "general-path", "default"] => {
            Ok(Command::SettingsSetGeneralPath { path: None })
        }
        ["settings", "general-path", path] => Ok(Command::SettingsSetGeneralPath {
            path: Some(absolute_or_supplied(path)),
        }),
        ["settings", "cleanup-now"] => Ok(Command::SettingsCleanupMerged { project: None }),
        ["settings", "cleanup-now", "--project", project] => Ok(Command::SettingsCleanupMerged {
            project: Some((*project).to_string()),
        }),
        ["model", "status"] => Ok(Command::ModelStatus),
        ["model", "install", "quality"] => Ok(Command::ModelInstall {
            model: ModelTier::Quality,
        }),
        ["ui"] => Ok(Command::UiShow),
        ["events"] => Ok(Command::Events {
            since_sequence: None,
        }),
        ["events", "--since", sequence] => Ok(Command::Events {
            since_sequence: Some(parse_number("sequence", sequence)?),
        }),
        ["project", "list"] => Ok(Command::ProjectList),
        ["project", "current"] => Ok(Command::ProjectCurrent),
        ["project", "select", project] => Ok(Command::ProjectSelect {
            project: (*project).to_string(),
        }),
        ["project", "add", path] => Ok(Command::ProjectAdd {
            path: absolute_or_supplied(path),
            name: None,
        }),
        ["project", "add", path, "--name", name] => Ok(Command::ProjectAdd {
            path: absolute_or_supplied(path),
            name: Some((*name).to_string()),
        }),
        ["project", "create", name] => Ok(Command::ProjectCreate {
            name: (*name).to_string(),
        }),
        ["project", "remove", project] => Ok(Command::ProjectRemove {
            project: (*project).to_string(),
        }),
        ["project", "refresh", project] => Ok(Command::ProjectRefresh {
            project: (*project).to_string(),
        }),
        ["record", "start"] => Ok(Command::RecordStart {
            project: None,
            note: None,
        }),
        ["record", "start", "--project", project] => Ok(Command::RecordStart {
            project: Some((*project).to_string()),
            note: None,
        }),
        ["record", "start", "--note", note] => Ok(Command::RecordStart {
            project: None,
            note: Some((*note).to_string()),
        }),
        ["record", "start", "--project", project, "--note", note]
        | ["record", "start", "--note", note, "--project", project] => Ok(Command::RecordStart {
            project: Some((*project).to_string()),
            note: Some((*note).to_string()),
        }),
        ["record", "stop"] => Ok(Command::RecordStop),
        ["record", "toggle"] => Ok(Command::RecordToggle),
        ["record", "status"] => Ok(Command::RecordStatus),
        ["recording", "show", selector] => Ok(Command::RecordingShow {
            recording: parse_selector(selector),
        }),
        ["recording", "open", selector] => Ok(Command::RecordingOpen {
            recording: parse_selector(selector),
        }),
        ["recording", "transcribe", selector] => Ok(Command::RecordingTranscribe {
            recording: parse_selector(selector),
        }),
        ["recording", "delete", selector] => Ok(Command::RecordingDelete {
            recording: parse_selector(selector),
        }),
        ["context", selector] => Ok(Command::Context {
            recording: parse_selector(selector),
            project: None,
            copy: false,
        }),
        ["context", selector, "--copy"] => Ok(Command::Context {
            recording: parse_selector(selector),
            project: None,
            copy: true,
        }),
        ["context", selector, "--project", project] => Ok(Command::Context {
            recording: parse_selector(selector),
            project: Some((*project).to_string()),
            copy: false,
        }),
        ["context", selector, "--project", project, "--copy"]
        | ["context", selector, "--copy", "--project", project] => Ok(Command::Context {
            recording: parse_selector(selector),
            project: Some((*project).to_string()),
            copy: true,
        }),
        ["annotate", "toggle"] => Ok(Command::AnnotationToggle),
        ["annotate", "enable"] => Ok(Command::AnnotationEnable),
        ["annotate", "disable"] => Ok(Command::AnnotationDisable),
        ["annotate", "undo"] => Ok(Command::AnnotationUndo),
        ["annotate", "clear"] => Ok(Command::AnnotationClear),
        ["annotate", "tool", tool] => Ok(Command::AnnotationTool {
            tool: AnnotationTool::from_str(tool)?,
        }),
        _ => Err(CliError(format!(
            "unknown or incomplete Dicta command: {}",
            args.join(" ")
        ))),
    }
}

fn parse_recording_list(arguments: &[&str]) -> Result<Command, CliError> {
    let mut project = None;
    let mut branch = None;
    let mut limit = None;
    let mut position = 0;
    while position < arguments.len() {
        let (slot, label) = match arguments[position] {
            "--project" => (&mut project, "project"),
            "--branch" => (&mut branch, "branch"),
            "--limit" => {
                let value = arguments
                    .get(position + 1)
                    .ok_or_else(|| CliError("--limit requires a value".to_string()))?;
                if limit.replace(parse_number("limit", value)?).is_some() {
                    return Err(CliError("--limit was provided more than once".to_string()));
                }
                position += 2;
                continue;
            }
            option => return Err(CliError(format!("unknown recording list option: {option}"))),
        };
        let value = arguments
            .get(position + 1)
            .ok_or_else(|| CliError(format!("--{label} requires a value")))?;
        if slot.replace((*value).to_string()).is_some() {
            return Err(CliError(format!("--{label} was provided more than once")));
        }
        position += 2;
    }
    Ok(Command::RecordingList {
        project,
        branch,
        limit,
    })
}

fn parse_number<T>(label: &str, value: &str) -> Result<T, CliError>
where
    T: FromStr,
{
    value
        .parse()
        .map_err(|_| CliError(format!("invalid {label}: {value}")))
}

fn parse_switch(label: &str, value: &str) -> Result<bool, CliError> {
    match value {
        "on" | "true" | "enabled" => Ok(true),
        "off" | "false" | "disabled" => Ok(false),
        _ => Err(CliError(format!(
            "invalid {label} value `{value}`; expected on or off"
        ))),
    }
}

fn parse_selector(value: &str) -> RecordingSelector {
    if value == "latest" {
        RecordingSelector::Latest
    } else {
        RecordingSelector::Id(value.to_string())
    }
}

fn absolute_or_supplied(path: &str) -> String {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path.to_string_lossy().into_owned()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(&path))
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }
}

impl FromStr for AnnotationTool {
    type Err = CliError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pen" => Ok(Self::Pen),
            "arrow" => Ok(Self::Arrow),
            "rectangle" => Ok(Self::Rectangle),
            "spotlight" => Ok(Self::Spotlight),
            _ => Err(CliError(format!("unknown annotation tool: {value}"))),
        }
    }
}
