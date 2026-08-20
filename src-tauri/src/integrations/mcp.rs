use crate::{path_string, McpStatus};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

pub(crate) fn install_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| "Could not locate the local application data folder".to_string())?;
    Ok(data_dir.join("Dicta").join("bin").join("dicta-mcp"))
}

pub(crate) fn atomic_install_binary(resource: &Path, target: &Path) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "Invalid MCP installation path".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create the Dicta MCP folder: {error}"))?;
    let staging = parent.join(format!(".dicta-mcp-installing-{}", std::process::id()));
    fs::copy(resource, &staging)
        .map_err(|error| format!("Could not stage the Dicta MCP server: {error}"))?;
    if let Err(error) = fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)) {
        let _ = fs::remove_file(&staging);
        return Err(format!(
            "Could not make the Dicta MCP server executable: {error}"
        ));
    }
    if let Err(error) = fs::rename(&staging, target) {
        let _ = fs::remove_file(&staging);
        return Err(format!(
            "Could not atomically install the Dicta MCP server: {error}"
        ));
    }
    Ok(())
}

pub(crate) fn install_binary(app: &AppHandle) -> Result<PathBuf, String> {
    let target = install_path()?;
    let resource = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Could not locate Dicta resources: {error}"))?
        .join("dicta-mcp");
    if resource.exists() {
        atomic_install_binary(&resource, &target)?;
    }
    if !target.exists() {
        return Err("The Dicta MCP server is not bundled in this build".to_string());
    }
    Ok(target)
}

fn find_codex_command() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join("codex");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/codex"));
        candidates.push(home.join(".bun/bin/codex"));
        candidates.push(home.join(".npm-global/bin/codex"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn codex_has_dicta(codex: &Path, executable: &Path) -> bool {
    let Ok(output) = std::process::Command::new(codex)
        .args(["mcp", "get", "dicta", "--json"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .ok()
        .and_then(|value| {
            value
                .pointer("/transport/command")
                .or_else(|| value.get("command"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .is_some_and(|command| command == path_string(executable))
}

pub(crate) fn register_codex_mcp(
    codex: &Path,
    executable: &Path,
    force_reload: bool,
) -> Result<(), String> {
    if !force_reload && codex_has_dicta(codex, executable) {
        return Ok(());
    }
    let _ = std::process::Command::new(codex)
        .args(["mcp", "remove", "dicta"])
        .output();
    if force_reload {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let output = std::process::Command::new(codex)
        .args(["mcp", "add", "dicta", "--"])
        .arg(executable)
        .output()
        .map_err(|error| format!("Could not configure Codex: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if message.is_empty() {
        "Codex could not save the Dicta MCP configuration".to_string()
    } else {
        message
    })
}

fn connected_status(executable: &Path, message: &str) -> McpStatus {
    McpStatus {
        installed: true,
        codex_configured: true,
        executable_path: path_string(executable),
        message: message.to_string(),
    }
}

#[tauri::command]
pub(crate) fn mcp_status(app: AppHandle) -> Result<McpStatus, String> {
    let executable = install_path()?;
    let installed = executable.is_file();
    let codex_configured = find_codex_command()
        .as_deref()
        .is_some_and(|codex| installed && codex_has_dicta(codex, &executable));
    Ok(McpStatus {
        installed,
        codex_configured,
        executable_path: path_string(&executable),
        message: if codex_configured {
            "Dicta is connected to Codex".to_string()
        } else if installed {
            "The Dicta MCP server is ready to connect".to_string()
        } else {
            let _ = app;
            "The Dicta MCP server is not installed".to_string()
        },
    })
}

#[tauri::command]
pub(crate) fn configure_codex_mcp(app: AppHandle) -> Result<McpStatus, String> {
    let executable = install_binary(&app)?;
    let codex = find_codex_command().ok_or_else(|| {
        format!(
            "Codex CLI was not found. Run: codex mcp add dicta -- {}",
            executable.display()
        )
    })?;
    register_codex_mcp(&codex, &executable, false)?;
    Ok(connected_status(
        &executable,
        "Dicta is connected to Codex.",
    ))
}

#[tauri::command]
pub(crate) fn restart_codex_mcp(app: AppHandle) -> Result<McpStatus, String> {
    let executable = install_binary(&app)?;
    let codex = find_codex_command().ok_or_else(|| {
        "Codex CLI was not found. Open Codex Settings → Plugins → MCPs to restart Dicta."
            .to_string()
    })?;
    register_codex_mcp(&codex, &executable, true)?;
    Ok(connected_status(
        &executable,
        "Dicta MCP restarted. Existing Codex tasks are reconnecting now.",
    ))
}
