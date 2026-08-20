#![cfg(unix)]

use dicta_control::{
    protocol::Response,
    socket::{ensure_private_runtime_dir, LocalClient, LocalServer, RequestPoll},
    Command, Event, EventEnvelope, ResponseEnvelope,
};
use std::{
    fs,
    io::Write,
    os::unix::fs::{symlink, FileTypeExt, PermissionsExt},
    os::unix::net::UnixStream,
    path::PathBuf,
    thread,
};

fn test_directory(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dicta-control-{name}-{}-{:?}",
        std::process::id(),
        thread::current().id()
    ))
}

#[test]
fn runtime_directory_and_socket_are_private() {
    let directory = test_directory("permissions");
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o777)).unwrap();

    ensure_private_runtime_dir(&directory).unwrap();
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let path = directory.join("control.sock");
    let server = LocalServer::bind(&path).unwrap();
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

    drop(server);
    assert!(!path.exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn runtime_directory_rejects_symlinks() {
    let directory = test_directory("symlink");
    let target = test_directory("symlink-target");
    let _ = fs::remove_file(&directory);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).unwrap();
    symlink(&target, &directory).unwrap();

    assert!(ensure_private_runtime_dir(&directory).is_err());

    fs::remove_file(directory).unwrap();
    fs::remove_dir_all(target).unwrap();
}

#[test]
fn client_correlates_response_while_buffering_events() {
    let directory = test_directory("client");
    let _ = fs::remove_dir_all(&directory);
    let path = directory.join("control.sock");
    let server = LocalServer::bind(&path).unwrap();

    let server_thread = thread::spawn(move || {
        let mut connection = server.accept().unwrap();
        let first = connection.read_request().unwrap().unwrap();
        let second = connection.read_request().unwrap().unwrap();
        assert_eq!(first.command, Command::Status);
        assert_eq!(second.command, Command::RecordStatus);
        connection
            .send_event(&EventEnvelope::new(Event::RecordingStarted {
                sequence: 1,
                recording_id: "rec-1".to_string(),
            }))
            .unwrap();
        connection
            .send_response(&ResponseEnvelope::success(second.id, Response::Accepted))
            .unwrap();
        connection
            .send_response(&ResponseEnvelope::success(first.id, Response::Accepted))
            .unwrap();
    });

    let mut client = LocalClient::connect(&path).unwrap();
    let first = client.send(Command::Status).unwrap();
    let second = client.send(Command::RecordStatus).unwrap();
    assert_eq!(client.wait(first).unwrap(), Response::Accepted);
    assert_eq!(client.wait(second).unwrap(), Response::Accepted);
    assert!(matches!(
        client.pop_event().unwrap().event,
        Event::RecordingStarted { sequence: 1, .. }
    ));

    server_thread.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn nonblocking_accept_distinguishes_idle_and_connected_states() {
    let directory = test_directory("nonblocking");
    let _ = fs::remove_dir_all(&directory);
    let path = directory.join("control.sock");
    let server = LocalServer::bind(&path).unwrap();
    server.set_nonblocking(true).unwrap();
    assert!(server.try_accept().unwrap().is_none());

    let client = LocalClient::connect(&path).unwrap();
    assert!(server.try_accept().unwrap().is_some());
    drop(client);
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn request_poll_retains_a_partial_frame_without_blocking() {
    let directory = test_directory("partial-frame");
    let _ = fs::remove_dir_all(&directory);
    let path = directory.join("control.sock");
    let server = LocalServer::bind(&path).unwrap();
    let mut client = UnixStream::connect(&path).unwrap();
    let mut connection = server.try_accept().unwrap().unwrap();

    client.write_all(br#"{"version":1,"id":1,"comm"#).unwrap();
    assert_eq!(connection.poll_request().unwrap(), RequestPoll::Pending);
    client.write_all(b"and\":\"status\"}\n").unwrap();
    let RequestPoll::Request(request) = connection.poll_request().unwrap() else {
        panic!("completed frame was not decoded");
    };
    assert_eq!(request.command, Command::Status);

    drop(client);
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}
