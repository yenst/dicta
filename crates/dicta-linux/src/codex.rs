use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const SERVER_NAME: &str = "dicta";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexMcpState {
    Connected,
    Disconnected,
    DifferentCommand,
    MissingCodex,
    MissingMcp,
    TransportFailure,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CodexMcpStatus {
    pub state: CodexMcpState,
    pub codex_path: Option<String>,
    pub mcp_path: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexProcessOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait CodexProcess: Send + Sync {
    fn run(&self, program: &Path, arguments: &[OsString]) -> io::Result<CodexProcessOutput>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCodexProcess;

impl CodexProcess for SystemCodexProcess {
    fn run(&self, program: &Path, arguments: &[OsString]) -> io::Result<CodexProcessOutput> {
        let output = Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .output()?;
        Ok(CodexProcessOutput {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

pub struct CodexMcpIntegration<P = SystemCodexProcess> {
    process: P,
    codex_path: Option<PathBuf>,
    mcp_path: Option<PathBuf>,
}

impl CodexMcpIntegration<SystemCodexProcess> {
    #[must_use]
    pub fn discover() -> Self {
        Self::new(
            SystemCodexProcess,
            executable_from_environment("DICTA_CODEX_BIN", "codex"),
            discover_mcp_path(),
        )
    }
}

impl<P> CodexMcpIntegration<P>
where
    P: CodexProcess,
{
    #[must_use]
    pub const fn new(process: P, codex_path: Option<PathBuf>, mcp_path: Option<PathBuf>) -> Self {
        Self {
            process,
            codex_path,
            mcp_path,
        }
    }

    #[must_use]
    pub fn status(&self) -> CodexMcpStatus {
        let Some(codex) = self.codex_path.as_deref() else {
            return self.base_status(
                CodexMcpState::MissingCodex,
                "Codex CLI was not found. Install Codex or set DICTA_CODEX_BIN.",
            );
        };
        let Some(mcp) = self.mcp_path.as_deref() else {
            return self.base_status(
                CodexMcpState::MissingMcp,
                "The packaged dicta-mcp executable was not found.",
            );
        };
        match self.registration(codex) {
            Ok(None) => self.base_status(
                CodexMcpState::Disconnected,
                "Dicta is not registered with Codex.",
            ),
            Ok(Some(registration)) if registration.matches(mcp) => self.base_status(
                CodexMcpState::Connected,
                "Codex uses this installed Dicta MCP executable.",
            ),
            Ok(Some(_)) => self.base_status(
                CodexMcpState::DifferentCommand,
                "Codex has a Dicta registration, but it points to another executable.",
            ),
            Err(message) => self.base_status(CodexMcpState::TransportFailure, &message),
        }
    }

    /// Registers the packaged Dicta MCP server when no conflicting entry exists.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when an executable is unavailable, Codex has a
    /// different `dicta` registration, or the typed Codex command fails.
    pub fn connect(&self) -> Result<CodexMcpStatus, String> {
        let (codex, mcp) = self.required_paths()?;
        match self.registration(codex)? {
            Some(registration) if registration.matches(mcp) => Ok(self.status()),
            Some(_) => Err(
                "Codex already has a different `dicta` MCP registration; use Restart Dicta MCP to replace it safely."
                    .to_owned(),
            ),
            None => {
                self.add_registration(codex, &Registration::stdio(mcp))?;
                self.require_connected()
            }
        }
    }

    /// Replaces the Dicta registration and restores the old value on failure.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when discovery, removal, addition, verification,
    /// or rollback fails.
    pub fn restart(&self) -> Result<CodexMcpStatus, String> {
        let (codex, mcp) = self.required_paths()?;
        let previous = self.registration(codex)?;
        if previous.is_some() {
            self.run_checked(codex, &["mcp", "remove", SERVER_NAME])?;
        }
        let desired = Registration::stdio(mcp);
        if let Err(add_error) = self.add_registration(codex, &desired) {
            let rollback = previous.as_ref().map_or(Ok(()), |registration| {
                self.add_registration(codex, registration)
            });
            return Err(match rollback {
                Ok(()) => format!(
                    "Could not restart Dicta MCP; the previous registration was restored: {add_error}"
                ),
                Err(rollback_error) => format!(
                    "Could not restart Dicta MCP and rollback also failed: {add_error}; {rollback_error}"
                ),
            });
        }
        self.require_connected()
    }

    fn required_paths(&self) -> Result<(&Path, &Path), String> {
        let codex = self
            .codex_path
            .as_deref()
            .ok_or_else(|| "Codex CLI was not found".to_owned())?;
        let mcp = self
            .mcp_path
            .as_deref()
            .ok_or_else(|| "The packaged dicta-mcp executable was not found".to_owned())?;
        Ok((codex, mcp))
    }

    fn require_connected(&self) -> Result<CodexMcpStatus, String> {
        let status = self.status();
        if status.state == CodexMcpState::Connected {
            Ok(status)
        } else {
            Err(format!(
                "Codex did not retain the Dicta MCP registration: {}",
                status.message
            ))
        }
    }

    fn base_status(&self, state: CodexMcpState, message: &str) -> CodexMcpStatus {
        CodexMcpStatus {
            state,
            codex_path: self.codex_path.as_deref().map(path_text),
            mcp_path: self.mcp_path.as_deref().map(path_text),
            message: message.to_owned(),
        }
    }

    fn registration(&self, codex: &Path) -> Result<Option<Registration>, String> {
        let output = self
            .process
            .run(
                codex,
                &[
                    OsString::from("mcp"),
                    OsString::from("get"),
                    OsString::from(SERVER_NAME),
                    OsString::from("--json"),
                ],
            )
            .map_err(|error| format!("Could not query Codex MCP registration: {error}"))?;
        if !output.success {
            let diagnostic = diagnostic(&output);
            if diagnostic.contains("No MCP server named") {
                return Ok(None);
            }
            return Err(format!("Codex MCP query failed: {diagnostic}"));
        }
        registration_document(&output.stdout)
            .map(|document| Some(document.transport))
            .map_err(|error| format!("Codex returned invalid MCP status JSON: {error}"))
    }

    fn add_registration(&self, codex: &Path, registration: &Registration) -> Result<(), String> {
        let mut arguments = vec![OsString::from("mcp"), OsString::from("add")];
        match registration {
            Registration::Stdio { command, args, env } => {
                for (key, value) in env {
                    arguments.push(OsString::from("--env"));
                    arguments.push(OsString::from(format!("{key}={value}")));
                }
                arguments.push(OsString::from(SERVER_NAME));
                arguments.push(OsString::from("--"));
                arguments.push(OsString::from(command));
                arguments.extend(args.iter().map(OsString::from));
            }
            Registration::Http { url } => {
                arguments.push(OsString::from(SERVER_NAME));
                arguments.push(OsString::from("--url"));
                arguments.push(OsString::from(url));
            }
        }
        let output = self
            .process
            .run(codex, &arguments)
            .map_err(|error| format!("Could not run Codex MCP registration: {error}"))?;
        if output.success {
            Ok(())
        } else {
            Err(format!(
                "Codex MCP registration failed: {}",
                diagnostic(&output)
            ))
        }
    }

    fn run_checked(&self, codex: &Path, arguments: &[&str]) -> Result<(), String> {
        let arguments = arguments.iter().map(OsString::from).collect::<Vec<_>>();
        let output = self
            .process
            .run(codex, &arguments)
            .map_err(|error| format!("Could not run Codex MCP command: {error}"))?;
        if output.success {
            Ok(())
        } else {
            Err(format!("Codex MCP command failed: {}", diagnostic(&output)))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct RegistrationDocument {
    transport: Registration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Registration {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, deserialize_with = "null_default")]
        env: BTreeMap<String, String>,
    },
    #[serde(alias = "streamable_http")]
    Http { url: String },
}

impl Registration {
    fn stdio(path: &Path) -> Self {
        Self::Stdio {
            command: path_text(path),
            args: Vec::new(),
            env: BTreeMap::new(),
        }
    }

    fn matches(&self, expected: &Path) -> bool {
        match self {
            Self::Stdio { command, args, env } => {
                args.is_empty() && env.is_empty() && paths_equal(Path::new(command), expected)
            }
            Self::Http { .. } => false,
        }
    }
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

fn registration_document(output: &[u8]) -> Result<RegistrationDocument, serde_json::Error> {
    if let Ok(document) = serde_json::from_slice(output) {
        return Ok(document);
    }

    // Launchers such as mise may print an informational line before forwarding
    // Codex's JSON. Parse the single JSON object without accepting arbitrary
    // trailing output as part of the status document.
    let start = output.iter().position(|byte| *byte == b'{').unwrap_or(0);
    let end = output
        .iter()
        .rposition(|byte| *byte == b'}')
        .map_or(output.len(), |index| index + 1);
    serde_json::from_slice(&output[start..end])
}

fn discover_mcp_path() -> Option<PathBuf> {
    if let Some(path) = explicit_executable("DICTA_MCP_BIN") {
        return Some(path);
    }
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    [
        directory.join("dicta-mcp"),
        directory.join("../share/Dicta/bin/dicta-mcp"),
        directory.join("../lib/Dicta/dicta-mcp"),
        directory.join("../lib/dicta/dicta-mcp"),
    ]
    .into_iter()
    .find_map(|path| regular_executable(&path))
}

fn executable_from_environment(variable: &str, fallback: &str) -> Option<PathBuf> {
    explicit_executable(variable).or_else(|| executable_on_path(fallback))
}

fn explicit_executable(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .and_then(|path| regular_executable(&path))
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find_map(|path| regular_executable(&path))
    })
}

fn regular_executable(path: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    path.canonicalize().ok()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    left.canonicalize()
        .ok()
        .zip(right.canonicalize().ok())
        .is_some_and(|(left, right)| left == right)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn diagnostic(output: &CodexProcessOutput) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if stdout.is_empty() {
            "command failed without a diagnostic".to_owned()
        } else {
            stdout
        }
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::OsStr,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct FakeProcess {
        state: Arc<Mutex<FakeState>>,
    }

    #[derive(Default)]
    struct FakeState {
        registration: Option<Registration>,
        calls: Vec<Vec<OsString>>,
        fail_next_add: bool,
        prefix_status_json: bool,
    }

    impl CodexProcess for FakeProcess {
        fn run(&self, _program: &Path, arguments: &[OsString]) -> io::Result<CodexProcessOutput> {
            let mut state = self.state.lock().unwrap();
            state.calls.push(arguments.to_vec());
            let words = arguments
                .iter()
                .map(|value| value.to_string_lossy())
                .collect::<Vec<_>>();
            let output = if words.get(1).is_some_and(|value| value == "get") {
                match &state.registration {
                    Some(registration) => CodexProcessOutput {
                        success: true,
                        stdout: {
                            let json = serde_json::to_vec(&serde_json::json!({
                                "transport": match registration {
                                    Registration::Stdio { command, args, env } => serde_json::json!({
                                        "type": "stdio", "command": command, "args": args, "env": env,
                                    }),
                                    Registration::Http { url } => serde_json::json!({
                                        "type": "http", "url": url,
                                    }),
                                }
                            }))
                            .unwrap();
                            if state.prefix_status_json {
                                [b"mise tools: codex@0.148.0\n".as_slice(), json.as_slice()]
                                    .concat()
                            } else {
                                json
                            }
                        },
                        stderr: Vec::new(),
                    },
                    None => CodexProcessOutput {
                        success: false,
                        stdout: Vec::new(),
                        stderr: b"No MCP server named 'dicta' found".to_vec(),
                    },
                }
            } else if words.get(1).is_some_and(|value| value == "remove") {
                state.registration = None;
                success()
            } else if words.get(1).is_some_and(|value| value == "add") {
                if state.fail_next_add {
                    state.fail_next_add = false;
                    CodexProcessOutput {
                        success: false,
                        stdout: Vec::new(),
                        stderr: b"injected add failure".to_vec(),
                    }
                } else {
                    let separator = words.iter().position(|word| word == "--").unwrap();
                    state.registration = Some(Registration::Stdio {
                        command: words[separator + 1].to_string(),
                        args: words[separator + 2..]
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                        env: BTreeMap::new(),
                    });
                    success()
                }
            } else {
                unreachable!()
            };
            Ok(output)
        }
    }

    fn success() -> CodexProcessOutput {
        CodexProcessOutput {
            success: true,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "dicta-codex-integration-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let codex = root.join("codex");
        let mcp = root.join("dicta-mcp");
        fs::write(&codex, "fixture").unwrap();
        fs::write(&mcp, "fixture").unwrap();
        (codex, mcp)
    }

    #[test]
    fn connect_uses_exact_argv_and_status_survives_a_new_integration() {
        let (codex, mcp) = fixture();
        let process = FakeProcess::default();
        let integration =
            CodexMcpIntegration::new(process.clone(), Some(codex.clone()), Some(mcp.clone()));
        assert_eq!(integration.status().state, CodexMcpState::Disconnected);
        assert_eq!(
            integration.connect().unwrap().state,
            CodexMcpState::Connected
        );
        let restarted = CodexMcpIntegration::new(process.clone(), Some(codex), Some(mcp));
        assert_eq!(restarted.status().state, CodexMcpState::Connected);
        let calls = &process.state.lock().unwrap().calls;
        assert!(calls.iter().any(|call| {
            call.len() == 5
                && call[0] == OsStr::new("mcp")
                && call[1] == OsStr::new("add")
                && call[2] == OsStr::new("dicta")
                && call[3] == OsStr::new("--")
                && Path::new(&call[4]) == restarted.mcp_path.as_deref().unwrap()
        }));
    }

    #[test]
    fn restart_rolls_back_the_previous_dicta_registration_only() {
        let (codex, mcp) = fixture();
        let process = FakeProcess::default();
        let previous = mcp.with_file_name("old-dicta-mcp");
        fs::write(&previous, "old").unwrap();
        process.state.lock().unwrap().registration = Some(Registration::stdio(&previous));
        process.state.lock().unwrap().fail_next_add = true;
        let integration = CodexMcpIntegration::new(process.clone(), Some(codex), Some(mcp));
        let error = integration.restart().unwrap_err();
        assert!(error.contains("previous registration was restored"));
        assert_eq!(
            process.state.lock().unwrap().registration,
            Some(Registration::stdio(&previous))
        );
    }

    #[test]
    fn status_accepts_launcher_information_before_codex_json() {
        let (codex, mcp) = fixture();
        let process = FakeProcess::default();
        process.state.lock().unwrap().registration = Some(Registration::stdio(&mcp));
        process.state.lock().unwrap().prefix_status_json = true;
        let integration = CodexMcpIntegration::new(process, Some(codex), Some(mcp));

        assert_eq!(integration.status().state, CodexMcpState::Connected);
    }
}
