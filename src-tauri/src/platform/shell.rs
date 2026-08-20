use std::{io::Write, path::Path, process::Command};

pub(crate) fn reveal(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path.display()));
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        if path.is_file() {
            command.arg("-R");
        }
        command.arg(path);
        command
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        });
        command
    };
    command
        .spawn()
        .map_err(|error| format!("Could not open the file manager: {error}"))?;
    Ok(())
}

pub(crate) fn copy_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("pbcopy");
    #[cfg(target_os = "linux")]
    let mut command = linux_clipboard_command()?;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err("Clipboard support is unavailable on this platform".to_string());

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut child = command
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not access the clipboard: {error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Could not open the clipboard".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("Could not write to the clipboard: {error}"))?;
        drop(stdin);
        let status = child
            .wait()
            .map_err(|error| format!("Clipboard command failed: {error}"))?;
        if !status.success() {
            return Err("Clipboard command did not complete successfully".to_string());
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn linux_clipboard_command() -> Result<Command, String> {
    use super::linux::environment::executable_exists;

    if std::env::var_os("WAYLAND_DISPLAY").is_some() && executable_exists("wl-copy") {
        return Ok(Command::new("wl-copy"));
    }
    if executable_exists("xclip") {
        let mut command = Command::new("xclip");
        command.args(["-selection", "clipboard"]);
        return Ok(command);
    }
    if executable_exists("xsel") {
        let mut command = Command::new("xsel");
        command.args(["--clipboard", "--input"]);
        return Ok(command);
    }
    Err("Clipboard support requires `wl-clipboard`, `xclip`, or `xsel` on Linux".to_string())
}
