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

fn main() {
    #[cfg(target_os = "linux")]
    configure_webkit_renderer();
    dicta_lib::run();
}
