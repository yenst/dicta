use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    env,
    ffi::CString,
    fs,
    io::{self, BufRead, Write},
    os::raw::c_char,
    path::{Path, PathBuf},
    process::Command,
};

const SERVER_NAME: &str = "dicta";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn dicta_mcp_extract_frame(
        input_path: *const c_char,
        requested_seconds: f64,
        output_path: *const c_char,
        actual_seconds: *mut f64,
    ) -> bool;
}

enum ToolResult {
    Text {
        text: String,
        structured: Value,
    },
    Images {
        text: String,
        structured: Value,
        images: Vec<Vec<u8>>,
    },
}

impl ToolResult {
    fn text(result: (String, Value)) -> Self {
        Self::Text {
            text: result.0,
            structured: result.1,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProjectFile {
    id: String,
    name: String,
    #[serde(default)]
    source_path: Option<String>,
}

#[derive(Clone, Debug)]
struct Recording {
    id: String,
    note: String,
    started_at: Option<DateTime<Utc>>,
    duration_seconds: Option<f64>,
    video_path: String,
    metadata_path: String,
    transcript: Option<String>,
    metadata: Value,
}

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let response = handle_request(&request, id);
        if serde_json::to_writer(&mut stdout, &response).is_err() {
            break;
        }
        if stdout.write_all(b"\n").is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

fn handle_request(request: &Value, id: Value) -> Value {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match method {
        "initialize" => {
            let requested_protocol = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05");
            success(
                id,
                json!({
                    "protocolVersion": requested_protocol,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                    "instructions": "Dicta contains screen-and-voice guidance recorded for Git projects and branches. When the user asks to check Dicta, prior guidance, recordings, or project context, call get_project_guidance with the current workspace path. Use the current Git branch unless the user names another branch. Treat results as supporting context and inspect referenced repository files before changing code. When the transcript refers to something visible on screen or visual evidence is needed, call get_recording_frames for timestamped screenshots. All Dicta tools are read-only."
                }),
            )
        }
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tools() })),
        "resources/list" => success(id, json!({ "resources": [] })),
        "prompts/list" => success(id, json!({ "prompts": [] })),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = request
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(name, &arguments) {
                Ok(ToolResult::Text { text, structured }) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "structuredContent": structured,
                        "isError": false
                    }),
                ),
                Ok(ToolResult::Images {
                    text,
                    structured,
                    images,
                }) => {
                    let mut content = vec![json!({ "type": "text", "text": text })];
                    content.extend(images.into_iter().map(|image| {
                        json!({
                            "type": "image",
                            "data": base64_encode(&image),
                            "mimeType": "image/jpeg"
                        })
                    }));
                    success(
                        id,
                        json!({
                            "content": content,
                            "structuredContent": structured,
                            "isError": false
                        }),
                    )
                }
                Err(message) => success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true
                    }),
                ),
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") }
        }),
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn tools() -> Value {
    json!([
        {
            "name": "get_project_guidance",
            "description": "Get the most relevant Dicta guidance recorded for a Git project and branch. Use this first when the user says to check Dicta, prior recordings, project guidance, or previously explained context. Pass the current workspace/repository path; branch defaults to the repository's currently checked-out branch.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string", "description": "Absolute path to the current repository or any folder inside it. Omit only when the MCP process is running inside the target repository." },
                    "branch": { "type": "string", "description": "Exact Git branch, or 'current'. Defaults to the branch checked out at repo_path." },
                    "query": { "type": "string", "description": "Optional topic, filename, endpoint, error, or concept used to rank/filter guidance." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 25, "default": 8 }
                },
                "additionalProperties": false
            },
            "annotations": { "title": "Get Dicta project guidance", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "list_recordings",
            "description": "List Dicta recordings for a Git project and branch in newest-first order. Use this to browse available captured guidance before opening a specific recording.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string", "description": "Absolute path to the repository or a folder inside it." },
                    "branch": { "type": "string", "description": "Exact branch or 'current'. Defaults to current." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 }
                },
                "additionalProperties": false
            },
            "annotations": { "title": "List Dicta recordings", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_recording",
            "description": "Read one Dicta recording's complete note, metadata, available transcript, and evidence paths. Obtain recording_id from get_project_guidance or list_recordings.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string", "description": "Absolute path to the repository or a folder inside it." },
                    "branch": { "type": "string", "description": "Exact branch or 'current'. Defaults to current." },
                    "recording_id": { "type": "string", "description": "Recording ID returned by another Dicta tool." }
                },
                "required": ["recording_id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Read a Dicta recording", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_recording_frames",
            "description": "Extract timestamped screenshots from one Dicta recording and return them inline as visual evidence. Use this when a transcript mentions something shown on screen, when exact UI or output matters, or when the user asks to inspect the video. Pass explicit timestamps when known; otherwise Dicta samples useful moments evenly across the recording.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string", "description": "Absolute path to the repository or a folder inside it." },
                    "branch": { "type": "string", "description": "Exact branch or 'current'. Defaults to current." },
                    "recording_id": { "type": "string", "description": "Recording ID returned by another Dicta tool." },
                    "timestamps_seconds": {
                        "type": "array",
                        "description": "Optional video timestamps in seconds. When omitted, Dicta samples the recording automatically.",
                        "items": { "type": "number", "minimum": 0 },
                        "maxItems": 8
                    },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 8, "default": 4 }
                },
                "required": ["recording_id"],
                "additionalProperties": false
            },
            "annotations": { "title": "View Dicta recording frames", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        }
    ])
}

fn call_tool(name: &str, arguments: &Value) -> Result<ToolResult, String> {
    match name {
        "get_project_guidance" => get_project_guidance(arguments).map(ToolResult::text),
        "list_recordings" => list_project_recordings(arguments).map(ToolResult::text),
        "get_recording" => get_recording(arguments).map(ToolResult::text),
        "get_recording_frames" => get_recording_frames(arguments),
        _ => Err(format!("Unknown Dicta tool: {name}")),
    }
}

fn get_project_guidance(arguments: &Value) -> Result<(String, Value), String> {
    let context = resolve_context(arguments)?;
    let query = string_arg(arguments, "query");
    let limit = limit_arg(arguments, 8, 25);
    let mut recordings = load_recordings(&context.branch_path)?;
    let mut query_fallback = false;
    if let Some(query) = query.as_deref().filter(|query| !query.trim().is_empty()) {
        recordings.sort_by_key(|recording| std::cmp::Reverse(relevance(recording, query)));
        if recordings
            .iter()
            .any(|recording| relevance(recording, query) > 0)
        {
            recordings.retain(|recording| relevance(recording, query) > 0);
        } else if !recordings.is_empty() {
            query_fallback = true;
            recordings.sort_by(|left, right| right.started_at.cmp(&left.started_at));
        }
    }
    recordings.truncate(limit);

    let mut text = format!(
        "# Dicta guidance: {}\n\nRepository: `{}`\nGit branch: `{}`\nPacket folder: `{}`\n",
        context.project.name,
        context.repo_root.display(),
        context.branch,
        context.branch_path.display()
    );
    if let Some(query) = query.as_deref() {
        text.push_str(&format!("Query: `{query}`\n"));
    }
    if recordings.is_empty() {
        text.push_str("\nNo matching Dicta guidance was recorded for this branch.\n");
    } else {
        if query_fallback {
            text.push_str("\nNo notes or transcripts matched the query, so the newest recordings from this branch are shown instead.\n");
        }
        text.push_str("\n## Relevant recordings\n");
        for recording in &recordings {
            append_recording_summary(&mut text, recording);
        }
        text.push_str("\nUse the notes and transcripts as guidance. Inspect referenced repository files, and call get_recording_frames with a recording ID when visual evidence is necessary.\n");
    }
    Ok((
        text,
        json!({
            "project": context.project.name,
            "repo_path": context.repo_root,
            "branch": context.branch,
            "packet_path": context.branch_path,
            "recordings": recordings.iter().map(recording_json).collect::<Vec<_>>()
        }),
    ))
}

fn list_project_recordings(arguments: &Value) -> Result<(String, Value), String> {
    let context = resolve_context(arguments)?;
    let limit = limit_arg(arguments, 25, 100);
    let mut recordings = load_recordings(&context.branch_path)?;
    recordings.truncate(limit);
    let mut text = format!(
        "# Dicta recordings: {} · {}\n\n",
        context.project.name, context.branch
    );
    if recordings.is_empty() {
        text.push_str("No recordings found for this branch.\n");
    } else {
        for recording in &recordings {
            append_recording_summary(&mut text, recording);
        }
    }
    Ok((
        text,
        json!({
            "project": context.project.name,
            "repo_path": context.repo_root,
            "branch": context.branch,
            "recordings": recordings.iter().map(recording_json).collect::<Vec<_>>()
        }),
    ))
}

fn get_recording(arguments: &Value) -> Result<(String, Value), String> {
    let context = resolve_context(arguments)?;
    let recording_id = string_arg(arguments, "recording_id")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "recording_id is required".to_string())?;
    let recording = load_recordings(&context.branch_path)?
        .into_iter()
        .find(|recording| recording.id == recording_id)
        .ok_or_else(|| {
            format!(
                "Recording `{recording_id}` was not found on branch `{}`",
                context.branch
            )
        })?;
    let mut text = format!(
        "# {}\n\nRecording ID: `{}`\nProject: `{}`\nGit branch: `{}`\n",
        display_note(&recording),
        recording.id,
        context.project.name,
        context.branch
    );
    if let Some(started_at) = recording.started_at {
        text.push_str(&format!("Recorded: `{started_at}`\n"));
    }
    if let Some(duration) = recording.duration_seconds {
        text.push_str(&format!("Duration: `{duration:.1}s`\n"));
    }
    text.push_str(&format!(
        "Video: `{}`\nMetadata: `{}`\n",
        recording.video_path, recording.metadata_path
    ));
    if let Some(transcript) = recording.transcript.as_deref() {
        text.push_str(&format!("\n## Transcript\n\n{transcript}\n"));
    } else {
        text.push_str("\nNo transcript is available yet. The note and video path are the available evidence.\n");
    }
    Ok((text, recording_json(&recording)))
}

fn get_recording_frames(arguments: &Value) -> Result<ToolResult, String> {
    let context = resolve_context(arguments)?;
    let recording_id = string_arg(arguments, "recording_id")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "recording_id is required".to_string())?;
    let recording = load_recordings(&context.branch_path)?
        .into_iter()
        .find(|recording| recording.id == recording_id)
        .ok_or_else(|| {
            format!(
                "Recording `{recording_id}` was not found on branch `{}`",
                context.branch
            )
        })?;
    if !Path::new(&recording.video_path).is_file() {
        return Err(format!(
            "The recording video is missing at `{}`",
            recording.video_path
        ));
    }

    let timestamps = requested_frame_timestamps(&recording, arguments)?;
    let output_dir = env::temp_dir().join("dicta-mcp-frames");
    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("Could not create the temporary frame folder: {error}"))?;

    let mut text = format!(
        "# Timestamped frames: {}\n\nRecording ID: `{}`\nVideo: `{}`\n",
        display_note(&recording),
        recording.id,
        recording.video_path
    );
    if recording.transcript.is_some() {
        text.push_str("Transcript excerpts below are approximate position-based context; the screenshot timestamps are exact.\n");
    }

    let safe_id = safe_file_component(&recording.id);
    let mut images = Vec::new();
    let mut frames = Vec::new();
    for requested_seconds in timestamps {
        let millis = (requested_seconds * 1000.0).round() as u64;
        let output_path = output_dir.join(format!("{safe_id}-{millis:010}.jpg"));
        let actual_seconds = extract_frame(
            Path::new(&recording.video_path),
            requested_seconds,
            &output_path,
        )?;
        let image = fs::read(&output_path)
            .map_err(|error| format!("Could not read extracted frame: {error}"))?;
        let timestamp = format_timestamp(actual_seconds);
        let excerpt = approximate_transcript_excerpt(&recording, actual_seconds);
        text.push_str(&format!(
            "\n## {timestamp}\n\nScreenshot: `{}`\n",
            output_path.display()
        ));
        if let Some(excerpt) = excerpt.as_deref() {
            text.push_str(&format!("Approximate transcript context: {excerpt}\n"));
        }
        frames.push(json!({
            "timestamp": timestamp,
            "timestamp_seconds": actual_seconds,
            "requested_seconds": requested_seconds,
            "image_path": output_path,
            "mime_type": "image/jpeg",
            "transcript_excerpt": excerpt,
            "transcript_timing": if recording.transcript.is_some() { "approximate_position" } else { "unavailable" }
        }));
        images.push(image);
    }

    Ok(ToolResult::Images {
        text,
        structured: json!({
            "project": context.project.name,
            "repo_path": context.repo_root,
            "branch": context.branch,
            "recording_id": recording.id,
            "video_path": recording.video_path,
            "frames": frames
        }),
        images,
    })
}

fn requested_frame_timestamps(
    recording: &Recording,
    arguments: &Value,
) -> Result<Vec<f64>, String> {
    let duration = recording
        .duration_seconds
        .filter(|value| value.is_finite() && *value > 0.0);
    if let Some(values) = arguments
        .get("timestamps_seconds")
        .and_then(Value::as_array)
    {
        if values.is_empty() {
            return Err("timestamps_seconds must contain at least one timestamp".to_string());
        }
        let mut timestamps = Vec::new();
        for value in values.iter().take(8) {
            let timestamp = value
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| "Every timestamp must be a finite number of seconds".to_string())?;
            let timestamp = duration
                .map(|duration| timestamp.min((duration - 0.05).max(0.0)))
                .unwrap_or(timestamp);
            if !timestamps
                .iter()
                .any(|existing: &f64| (*existing - timestamp).abs() < 0.01)
            {
                timestamps.push(timestamp);
            }
        }
        return Ok(timestamps);
    }

    let duration = duration.ok_or_else(|| {
        "This recording has no duration metadata. Pass timestamps_seconds explicitly.".to_string()
    })?;
    let limit = limit_arg(arguments, 4, 8);
    if duration <= 1.0 {
        return Ok(vec![0.0]);
    }
    let count = limit.min((duration / 4.0).ceil() as usize).max(1);
    Ok((1..=count)
        .map(|index| duration * index as f64 / (count + 1) as f64)
        .collect())
}

#[cfg(target_os = "macos")]
fn extract_frame(video_path: &Path, seconds: f64, output_path: &Path) -> Result<f64, String> {
    let input = CString::new(video_path.to_string_lossy().as_bytes())
        .map_err(|_| "The video path contains an unsupported character".to_string())?;
    let output = CString::new(output_path.to_string_lossy().as_bytes())
        .map_err(|_| "The frame path contains an unsupported character".to_string())?;
    let mut actual_seconds = seconds;
    let extracted = unsafe {
        dicta_mcp_extract_frame(
            input.as_ptr(),
            seconds,
            output.as_ptr(),
            &mut actual_seconds,
        )
    };
    if extracted && output_path.is_file() {
        Ok(actual_seconds)
    } else {
        Err(format!(
            "Could not extract a screenshot at {} from `{}`",
            format_timestamp(seconds),
            video_path.display()
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn extract_frame(_video_path: &Path, _seconds: f64, _output_path: &Path) -> Result<f64, String> {
    Err("Timestamped frame extraction currently supports macOS only".to_string())
}

fn approximate_transcript_excerpt(recording: &Recording, seconds: f64) -> Option<String> {
    let transcript = recording.transcript.as_deref()?;
    let duration = recording.duration_seconds.filter(|value| *value > 0.0)?;
    let words = transcript.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return None;
    }
    let center = ((seconds / duration).clamp(0.0, 1.0) * words.len() as f64) as usize;
    let start = center.saturating_sub(24);
    let end = (center + 25).min(words.len());
    let mut excerpt = words[start..end].join(" ");
    if start > 0 {
        excerpt.insert_str(0, "…");
    }
    if end < words.len() {
        excerpt.push('…');
    }
    Some(excerpt)
}

fn format_timestamp(seconds: f64) -> String {
    let total_tenths = (seconds.max(0.0) * 10.0).round() as u64;
    let hours = total_tenths / 36_000;
    let minutes = (total_tenths / 600) % 60;
    let secs = (total_tenths / 10) % 60;
    let tenths = total_tenths % 10;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}.{tenths}")
    } else {
        format!("{minutes:02}:{secs:02}.{tenths}")
    }
}

fn safe_file_component(value: &str) -> String {
    let component = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let component = component.trim_matches('-');
    if component.is_empty() {
        "recording".to_string()
    } else {
        component.to_string()
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

struct Context {
    repo_root: PathBuf,
    branch: String,
    branch_path: PathBuf,
    project: ProjectFile,
}

fn resolve_context(arguments: &Value) -> Result<Context, String> {
    let requested_path = string_arg(arguments, "repo_path")
        .map(PathBuf::from)
        .unwrap_or(
            env::current_dir()
                .map_err(|error| format!("Could not determine current directory: {error}"))?,
        );
    let repo_root = git_root(&requested_path)?;
    let branch = match string_arg(arguments, "branch").as_deref() {
        None | Some("") | Some("current") => git_branch(&repo_root)?,
        Some(branch) => branch.to_string(),
    };
    let local_storage = repo_root.join(".dicta");
    let local_project_path = local_storage.join("project.json");
    let (project, branch_path) = if local_project_path.is_file() {
        let content = fs::read_to_string(&local_project_path).map_err(|error| {
            format!(
                "Dicta found repository-local storage at `{}`, but could not read it: {error}",
                local_storage.display()
            )
        })?;
        let project = serde_json::from_str::<ProjectFile>(&content)
            .map_err(|error| format!("Invalid Dicta project metadata: {error}"))?;
        let branch_path = local_storage
            .join("branches")
            .join(branch_folder_name(&branch));
        (project, branch_path)
    } else {
        let storage_root = dicta_root()?;
        let project = find_project(&storage_root, &repo_root).map_err(|legacy_error| {
            format!(
                "No repository-local Dicta storage was found at `{}`. Open Dicta 0.6.2 or newer and link this Git project once to make its recordings agent-accessible. Legacy lookup also failed: {legacy_error}",
                local_storage.display()
            )
        })?;
        let branch_path = storage_root
            .join(&project.id)
            .join("branches")
            .join(branch_folder_name(&branch));
        (project, branch_path)
    };
    Ok(Context {
        repo_root,
        branch,
        branch_path,
        project,
    })
}

fn dicta_root() -> Result<PathBuf, String> {
    if let Ok(path) = env::var("DICTA_HOME") {
        return Ok(PathBuf::from(path));
    }
    let documents =
        dirs::document_dir().ok_or_else(|| "Could not locate the Documents folder".to_string())?;
    let current = documents.join("Dicta");
    let legacy = documents.join("PromptReel");
    if current.exists() || !legacy.exists() {
        Ok(current)
    } else {
        Ok(legacy)
    }
}

fn find_project(storage_root: &Path, repo_root: &Path) -> Result<ProjectFile, String> {
    let entries = fs::read_dir(storage_root).map_err(|_| {
        format!(
            "Dicta storage was not found at `{}`. Open Dicta and link this Git project first.",
            storage_root.display()
        )
    })?;
    for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
        let path = entry.path().join("project.json");
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(project) = serde_json::from_str::<ProjectFile>(&content) else {
            continue;
        };
        let Some(source) = project.source_path.as_ref() else {
            continue;
        };
        let source_path = PathBuf::from(source);
        let normalized = source_path.canonicalize().unwrap_or(source_path);
        if normalized == repo_root {
            return Ok(project);
        }
    }
    Err(format!(
        "No Dicta project is linked to `{}`. Open Dicta and choose Link project folder.",
        repo_root.display()
    ))
}

fn git_output(path: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not run Git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{}` is not inside a Git working copy",
            path.display()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_root(path: &Path) -> Result<PathBuf, String> {
    PathBuf::from(git_output(path, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .map_err(|error| format!("Could not resolve Git root: {error}"))
}

fn git_branch(path: &Path) -> Result<String, String> {
    match git_output(path, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if !branch.is_empty() => Ok(branch),
        _ => Ok(format!(
            "detached@{}",
            git_output(path, &["rev-parse", "--short", "HEAD"])?
        )),
    }
}

fn branch_folder_name(branch: &str) -> String {
    let mut folder = String::new();
    for character in branch.chars() {
        match character {
            '/' => folder.push_str("__"),
            character
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') =>
            {
                folder.push(character)
            }
            _ => folder.push('-'),
        }
    }
    let folder = folder.trim_matches(['.', '-']).to_string();
    if folder.is_empty() {
        "unknown".to_string()
    } else {
        folder
    }
}

fn load_recordings(branch_path: &Path) -> Result<Vec<Recording>, String> {
    let recordings_root = branch_path.join("recordings");
    if !recordings_root.exists() {
        return Ok(Vec::new());
    }
    let mut metadata_files = Vec::new();
    for day in fs::read_dir(&recordings_root)
        .map_err(|error| format!("Could not read recordings: {error}"))?
        .flatten()
    {
        if !day.path().is_dir() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(day.path()) {
            metadata_files.extend(
                entries.flatten().map(|entry| entry.path()).filter(|path| {
                    path.extension().and_then(|value| value.to_str()) == Some("json")
                }),
            );
        }
    }
    let mut recordings = metadata_files
        .into_iter()
        .filter_map(read_recording)
        .collect::<Vec<_>>();
    recordings.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(recordings)
}

fn read_recording(metadata_path: PathBuf) -> Option<Recording> {
    let content = fs::read_to_string(&metadata_path).ok()?;
    let metadata = serde_json::from_str::<Value>(&content).ok()?;
    let id = metadata.get("id")?.as_str()?.to_string();
    let note = metadata
        .get("note")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let started_at = metadata
        .get("started_at")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok());
    let duration_seconds = metadata.get("duration_seconds").and_then(Value::as_f64);
    let recorded_video_path = metadata
        .get("video_path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let local_video_path = metadata_path.with_extension("mp4");
    let video_path = if local_video_path.is_file() {
        local_video_path.to_string_lossy().into_owned()
    } else {
        recorded_video_path
    };
    let transcript = metadata
        .get("transcript")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| read_transcript(&metadata, &metadata_path));
    Some(Recording {
        id,
        note,
        started_at,
        duration_seconds,
        video_path,
        metadata_path: metadata_path.to_string_lossy().into_owned(),
        transcript,
        metadata,
    })
}

fn read_transcript(metadata: &Value, metadata_path: &Path) -> Option<String> {
    if let Some(path) = metadata.get("transcript_path").and_then(Value::as_str) {
        if let Ok(content) = fs::read_to_string(path) {
            return Some(content);
        }
    }
    let stem = metadata_path.file_stem()?.to_str()?;
    for file_name in [format!("{stem}.transcript.md"), format!("{stem}.md")] {
        if let Ok(content) = fs::read_to_string(metadata_path.with_file_name(file_name)) {
            return Some(content);
        }
    }
    None
}

fn relevance(recording: &Recording, query: &str) -> usize {
    let haystack = format!(
        "{} {}",
        recording.note,
        recording.transcript.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    let query = query.to_lowercase();
    if haystack.contains(&query) {
        return 100 + query.len();
    }
    query
        .split_whitespace()
        .filter(|term| term.len() > 1 && haystack.contains(term))
        .count()
}

fn append_recording_summary(output: &mut String, recording: &Recording) {
    output.push_str(&format!(
        "\n- **{}**\n  - Recording ID: `{}`\n",
        display_note(recording),
        recording.id
    ));
    if let Some(started_at) = recording.started_at {
        output.push_str(&format!("  - Recorded: `{started_at}`\n"));
    }
    if let Some(duration) = recording.duration_seconds {
        output.push_str(&format!("  - Duration: `{duration:.1}s`\n"));
    }
    output.push_str(&format!(
        "  - Video: `{}`\n  - Metadata: `{}`\n",
        recording.video_path, recording.metadata_path
    ));
    if let Some(transcript) = recording.transcript.as_deref() {
        let preview = transcript
            .split_whitespace()
            .take(80)
            .collect::<Vec<_>>()
            .join(" ");
        output.push_str(&format!(
            "  - Transcript preview: {}{}\n",
            preview,
            if transcript.split_whitespace().count() > 80 {
                "…"
            } else {
                ""
            }
        ));
    } else {
        output.push_str("  - Transcript: not available\n");
    }
}

fn recording_json(recording: &Recording) -> Value {
    json!({
        "id": recording.id,
        "note": recording.note,
        "started_at": recording.started_at,
        "duration_seconds": recording.duration_seconds,
        "video_path": recording.video_path,
        "metadata_path": recording.metadata_path,
        "transcript": recording.transcript,
        "metadata": recording.metadata
    })
}

fn display_note(recording: &Recording) -> &str {
    if recording.note.trim().is_empty() {
        "Untitled recording"
    } else {
        &recording.note
    }
}

fn string_arg(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn limit_arg(arguments: &Value, default: usize, maximum: usize) -> usize {
    arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
        .clamp(1, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_paths_match_dicta_app() {
        assert_eq!(branch_folder_name("feature/oauth"), "feature__oauth");
        assert_eq!(branch_folder_name("main"), "main");
    }

    #[test]
    fn query_relevance_uses_notes_and_transcripts() {
        let recording = Recording {
            id: "one".into(),
            note: "Authentication edge cases".into(),
            started_at: None,
            duration_seconds: None,
            video_path: String::new(),
            metadata_path: String::new(),
            transcript: Some("The refresh token endpoint retries once".into()),
            metadata: json!({}),
        };
        assert!(relevance(&recording, "authentication") > 0);
        assert!(relevance(&recording, "refresh endpoint") > 0);
        assert_eq!(relevance(&recording, "billing"), 0);
    }

    #[test]
    fn formats_frame_timestamps() {
        assert_eq!(format_timestamp(18.24), "00:18.2");
        assert_eq!(format_timestamp(138.0), "02:18.0");
        assert_eq!(format_timestamp(3_723.94), "01:02:03.9");
    }

    #[test]
    fn encodes_frame_bytes_as_base64() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn automatic_frames_are_evenly_spaced() {
        let recording = Recording {
            id: "one".into(),
            note: String::new(),
            started_at: None,
            duration_seconds: Some(100.0),
            video_path: String::new(),
            metadata_path: String::new(),
            transcript: None,
            metadata: json!({}),
        };
        assert_eq!(
            requested_frame_timestamps(&recording, &json!({ "limit": 4 })).unwrap(),
            vec![20.0, 40.0, 60.0, 80.0]
        );
        assert_eq!(
            safe_file_component("feature/oauth demo"),
            "feature-oauth-demo"
        );
    }

    #[test]
    fn resolves_repository_local_dicta_storage() {
        let root = std::env::temp_dir().join(format!(
            "dicta-mcp-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        assert!(Command::new("git")
            .args(["init", "-b", "main"])
            .arg(&root)
            .status()
            .unwrap()
            .success());
        let recording_day = root.join(".dicta/branches/main/recordings/2026-08-13");
        fs::create_dir_all(&recording_day).unwrap();
        fs::write(
            root.join(".dicta/project.json"),
            json!({
                "id": "securex",
                "name": "Securex",
                "source_path": root.to_string_lossy()
            })
            .to_string(),
        )
        .unwrap();
        let local_video = recording_day.join("14-25-09.mp4");
        fs::write(&local_video, "video").unwrap();
        fs::write(
            recording_day.join("14-25-09.json"),
            json!({
                "id": "securex-quotas",
                "note": "",
                "started_at": "2026-08-13T12:25:09Z",
                "video_path": "/Users/jens/Documents/PromptReel/peepel/14-25-09.mp4"
            })
            .to_string(),
        )
        .unwrap();

        let context = resolve_context(&json!({ "repo_path": root.to_string_lossy() })).unwrap();
        assert_eq!(context.project.name, "Securex");
        assert_eq!(context.branch, "main");
        assert_eq!(
            context.branch_path,
            root.canonicalize().unwrap().join(".dicta/branches/main")
        );
        let (text, structured) = get_project_guidance(&json!({
            "repo_path": root.to_string_lossy(),
            "query": "Securex quotas"
        }))
        .unwrap();
        assert!(text.contains("newest recordings"));
        assert_eq!(structured["recordings"].as_array().unwrap().len(), 1);
        assert_eq!(
            structured["recordings"][0]["video_path"].as_str().unwrap(),
            root.canonicalize()
                .unwrap()
                .join(".dicta/branches/main/recordings/2026-08-13/14-25-09.mp4")
                .to_string_lossy()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
