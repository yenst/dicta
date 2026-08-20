use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn init_git(path: &Path) {
    fs::create_dir_all(path).unwrap();
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(target_os = "linux")]
fn write_video_fixture(path: &Path) {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x1a1b26:s=320x180:r=10:d=2",
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(path)
        .output()
        .expect("ffmpeg is required by the native Linux MCP frame tool");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn packaged_binary_preserves_stdio_protocol_and_lists_native_tools() {
    let fixture = tempfile::tempdir().unwrap();
    let storage_root = fixture.path().join("Dicta");
    let repo = fixture.path().join("repo");
    init_git(&repo);
    let project = dicta_core::ProjectFile {
        id: dicta_core::ProjectId::new("demo").unwrap(),
        name: "Demo".to_owned(),
        created_at: std::time::UNIX_EPOCH.into(),
        source_path: Some(repo.to_string_lossy().into_owned()),
    };
    dicta_core::storage::write_json_atomic(&storage_root.join("demo/project.json"), &project)
        .unwrap();
    let local = repo.join(".dicta");
    dicta_core::storage::write_json_atomic(&local.join("project.json"), &project).unwrap();
    let day = local.join("recordings/2026-08-20");
    fs::create_dir_all(&day).unwrap();
    #[cfg(target_os = "linux")]
    write_video_fixture(&day.join("take-one.mp4"));
    fs::write(
        day.join("take-one.json"),
        json!({
            "id": "take-one",
            "note": "Native MCP fixture",
            "recording_scope": "repository",
            "duration_seconds": 2.0,
            "transcript": "The filesystem catalog is the source of truth.",
            "transcript_segments": [{
                "start_seconds": 0.0,
                "end_seconds": 2.0,
                "text": "The filesystem catalog is the source of truth."
            }]
        })
        .to_string(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_dicta-mcp"))
        .env("DICTA_HOME", &storage_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{{").unwrap();
    writeln!(
        stdin,
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "cargo-smoke", "version": "1" }
            }
        })
    )
    .unwrap();
    writeln!(
        stdin,
        "{}",
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })
    )
    .unwrap();
    for request in [
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "list_projects", "arguments": { "repo_path": repo } }
        }),
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "get_current_project", "arguments": { "repo_path": repo } }
        }),
        json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "get_project_guidance", "arguments": { "repo_path": repo, "branch": "main", "query": "source of truth" } }
        }),
        json!({
            "jsonrpc": "2.0", "id": 6, "method": "tools/call",
            "params": { "name": "list_recordings", "arguments": { "repo_path": repo, "branch": "main" } }
        }),
        json!({
            "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "get_recording", "arguments": { "repo_path": repo, "branch": "main", "recording_id": "take-one" } }
        }),
        json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": { "name": "get_recording_context", "arguments": { "repo_path": repo, "branch": "main", "recording_id": "take-one" } }
        }),
        json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": { "name": "get_recording_frames", "arguments": { "repo_path": repo, "branch": "main", "recording_id": "take-one", "timestamps_seconds": [0.5] } }
        }),
    ] {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 10);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["result"]["serverInfo"]["name"], "dicta");
    let names = responses[2]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        "list_projects",
        "get_current_project",
        "get_project_guidance",
        "list_recordings",
        "get_recording",
        "get_recording_context",
        "get_recording_frames",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    assert_eq!(
        responses[3]["result"]["structuredContent"]["current_project_id"],
        "demo"
    );
    assert_eq!(
        responses[4]["result"]["structuredContent"]["project"]["id"],
        "demo"
    );
    assert_eq!(
        responses[5]["result"]["structuredContent"]["recordings"][0]["id"],
        "take-one"
    );
    assert_eq!(
        responses[6]["result"]["structuredContent"]["recordings"][0]["id"],
        "take-one"
    );
    assert_eq!(
        responses[7]["result"]["structuredContent"]["transcript"],
        "The filesystem catalog is the source of truth."
    );
    assert!(responses[8]["result"]["structuredContent"]["context"]
        .as_str()
        .unwrap()
        .contains("repository-wide"));
    #[cfg(target_os = "linux")]
    {
        assert_ne!(responses[9]["result"]["isError"], true);
        assert_eq!(
            responses[9]["result"]["structuredContent"]["frames"][0]["mime_type"],
            "image/jpeg"
        );
        assert_eq!(responses[9]["result"]["content"][1]["type"], "image");
        assert_eq!(
            responses[9]["result"]["content"][1]["mimeType"],
            "image/jpeg"
        );
        assert!(responses[9]["result"]["content"][1]["data"]
            .as_str()
            .unwrap()
            .starts_with("/9j/"));
        assert!(responses[9]["result"]["structuredContent"]["frames"][0]
            .get("path")
            .is_none());
    }
}
