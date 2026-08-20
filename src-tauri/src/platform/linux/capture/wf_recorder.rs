use super::{silent_command, RecorderProcess};
use crate::platform::linux::environment::executable_exists;
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

pub(super) fn start(output_path: &Path) -> Result<RecorderProcess, String> {
    let selected_output = if executable_exists("slurp") {
        let selection = Command::new("slurp")
            .args(["-o", "-f", "%o"])
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("Could not open the display picker: {error}"))?;
        if !selection.status.success() {
            let message = String::from_utf8_lossy(&selection.stderr);
            return Err(if message.trim().is_empty() {
                "Display selection was cancelled".to_string()
            } else {
                format!("Could not select a display: {}", message.trim())
            });
        }
        Some(parse_selected_output(&selection.stdout)?)
    } else {
        None
    };

    let mut command = silent_command("wf-recorder");
    command.args([
        "--audio",
        "--codec",
        "libx264",
        "--codec-param",
        "preset=ultrafast",
        "--codec-param",
        "crf=23",
        "--pixel-format",
        "yuv420p",
        "--audio-codec",
        "aac",
        "--framerate",
        "30",
    ]);
    command.args(output_args(output_path, selected_output.as_deref()));
    let child = command
        .spawn()
        .map_err(|error| format!("Could not start wf-recorder: {error}"))?;
    Ok(RecorderProcess::Direct {
        child,
        output_path: output_path.to_path_buf(),
        stop_with_interrupt: true,
        normalize_after: true,
    })
}

fn output_args(output_path: &Path, selected_output: Option<&str>) -> Vec<OsString> {
    let mut arguments = Vec::with_capacity(if selected_output.is_some() { 4 } else { 2 });
    if let Some(output) = selected_output {
        arguments.push("--output".into());
        arguments.push(output.into());
    }
    arguments.push("--file".into());
    arguments.push(output_path.as_os_str().to_owned());
    arguments
}

fn parse_selected_output(output: &[u8]) -> Result<String, String> {
    let output = String::from_utf8_lossy(output).trim().to_string();
    if output.is_empty() {
        return Err("Display selection was cancelled".to_string());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_picker_output_is_trimmed_and_required() {
        assert_eq!(parse_selected_output(b"DP-1\n").unwrap(), "DP-1");
        assert_eq!(
            parse_selected_output(b"  ").unwrap_err(),
            "Display selection was cancelled"
        );
    }

    #[test]
    fn selected_output_precedes_the_file_argument() {
        assert_eq!(
            output_args(Path::new("/tmp/recording.mp4"), Some("DP-1")),
            ["--output", "DP-1", "--file", "/tmp/recording.mp4"]
                .map(OsString::from)
                .to_vec()
        );
    }
}
