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
fn send_control_command() -> bool {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    if !std::env::args().any(|argument| argument == "--toggle-recording") {
        return false;
    }
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    match UnixStream::connect(runtime_dir.join("dicta-control.sock"))
        .and_then(|mut stream| stream.write_all(b"toggle-recording\n"))
    {
        Ok(()) => {}
        Err(error) => eprintln!("Could not contact the running Dicta app: {error}"),
    }
    true
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
