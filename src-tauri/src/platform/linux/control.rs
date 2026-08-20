use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
};
use tauri::AppHandle;

pub(crate) fn start(app: AppHandle, handle_command: fn(&AppHandle, &str)) -> Result<(), String> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let socket_path = runtime_dir.join("dicta-control.sock");
    if socket_path.exists() {
        fs::remove_file(&socket_path)
            .map_err(|error| format!("Could not replace the Dicta control socket: {error}"))?;
    }
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("Could not open the Dicta control socket: {error}"))?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not secure the Dicta control socket: {error}"))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut command = String::new();
            if BufReader::new(stream).read_line(&mut command).is_ok() {
                handle_command(&app, command.trim());
            }
        }
    });
    Ok(())
}
