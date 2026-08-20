#[cfg(target_os = "linux")]
fn uses_nvidia_graphics() -> bool {
    std::fs::read_dir("/sys/class/drm")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            std::fs::read_to_string(entry.path().join("device/vendor"))
                .is_ok_and(|vendor| vendor.trim().eq_ignore_ascii_case("0x10de"))
        })
}

#[cfg(target_os = "linux")]
fn configure_webkit_renderer() {
    let gdk_allows_wayland = match std::env::var("GDK_BACKEND") {
        Ok(value) => value != "x11",
        Err(_) => true,
    };
    let native_wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland")
        && gdk_allows_wayland;
    if native_wayland
        && uses_nvidia_graphics()
        && std::env::var_os("DICTA_ENABLE_WEBKIT_DMABUF").is_none()
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        // WebKitGTK's DMA-BUF renderer can terminate with a Wayland protocol
        // error on NVIDIA. This must be selected before GTK is initialized.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(target_os = "linux")]
fn control_command(arguments: &[String]) -> Option<String> {
    if arguments
        .iter()
        .any(|argument| argument == "--toggle-recording")
    {
        return Some("toggle-recording".to_string());
    }
    if arguments.iter().any(|argument| argument == "--show") {
        return Some("show".to_string());
    }
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--show-recording")
    {
        return arguments.get(index + 1).zip(arguments.get(index + 2)).map(
            |(project_id, recording_id)| format!("show-recording\t{project_id}\t{recording_id}"),
        );
    }
    arguments
        .iter()
        .position(|argument| argument == "--show-project")
        .and_then(|index| arguments.get(index + 1))
        .map(|project_id| format!("show-project\t{project_id}"))
}

#[cfg(target_os = "linux")]
fn send_control_command() -> bool {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let arguments = std::env::args().collect::<Vec<_>>();
    let Some(command) = control_command(&arguments) else {
        return false;
    };
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    match UnixStream::connect(runtime_dir.join("dicta-control.sock"))
        .and_then(|mut stream| stream.write_all(format!("{command}\n").as_bytes()))
    {
        Ok(()) => true,
        Err(error) if command == "toggle-recording" => {
            eprintln!("Could not contact the running Dicta app: {error}");
            true
        }
        Err(_) => false,
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    if send_control_command() {
        return;
    }
    #[cfg(target_os = "linux")]
    configure_webkit_renderer();
    dicta_lib::run();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::control_command;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn recording_control_command_keeps_project_and_recording_ids() {
        assert_eq!(
            control_command(&arguments(&[
                "dicta",
                "--show-recording",
                "dicta",
                "20260819-20-18-43"
            ])),
            Some("show-recording\tdicta\t20260819-20-18-43".to_string())
        );
    }

    #[test]
    fn show_control_command_focuses_an_existing_app() {
        assert_eq!(
            control_command(&arguments(&["dicta", "--show"])),
            Some("show".to_string())
        );
    }
}
