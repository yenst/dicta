use std::env;

#[derive(Clone, Copy)]
pub(super) struct CaptureEnvironment {
    pub(super) is_wayland: bool,
    pub(super) is_kde: bool,
    pub(super) spectacle_available: bool,
    pub(super) wf_recorder_available: bool,
}

impl CaptureEnvironment {
    pub(super) fn current() -> Self {
        Self {
            is_wayland: is_wayland_session(),
            is_kde: env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("kde"),
            spectacle_available: executable_exists("spectacle"),
            wf_recorder_available: executable_exists("wf-recorder"),
        }
    }
}

pub(crate) fn executable_exists(name: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|directory| directory.join(name).is_file()))
}

fn is_wayland_session() -> bool {
    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    if session_type.eq_ignore_ascii_case("x11") {
        return false;
    }
    if env::var("GDK_BACKEND").is_ok_and(|backend| backend.eq_ignore_ascii_case("x11")) {
        return false;
    }
    session_type.eq_ignore_ascii_case("wayland") || env::var_os("WAYLAND_DISPLAY").is_some()
}
