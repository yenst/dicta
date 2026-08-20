mod frames;
mod guidance;
mod projects;
mod recording;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

pub(crate) enum ToolResult {
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

pub(crate) fn call(name: &str, arguments: Value) -> Result<ToolResult, String> {
    match name {
        "get_project_guidance" => guidance::get(parse(name, arguments)?).map(ToolResult::text),
        "list_projects" => projects::list(parse(name, arguments)?).map(ToolResult::text),
        "get_current_project" => projects::current(parse(name, arguments)?).map(ToolResult::text),
        "list_recordings" => guidance::list(parse(name, arguments)?).map(ToolResult::text),
        "get_recording" => recording::get(parse(name, arguments)?).map(ToolResult::text),
        "get_recording_context" => {
            recording::context(parse(name, arguments)?).map(ToolResult::text)
        }
        "get_recording_frames" => frames::get(parse(name, arguments)?),
        _ => Err(format!("Unknown Dicta tool: {name}")),
    }
}

fn parse<T: DeserializeOwned>(name: &str, arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments)
        .map_err(|error| format!("Invalid arguments for `{name}`: {error}"))
}

pub(crate) fn checked_limit(
    value: Option<u64>,
    default: usize,
    maximum: usize,
) -> Result<usize, String> {
    let value = value.unwrap_or(default as u64);
    if value == 0 || value > maximum as u64 {
        return Err(format!("limit must be between 1 and {maximum}"));
    }
    usize::try_from(value).map_err(|_| "limit is too large for this platform".to_string())
}

pub(crate) fn definitions() -> Value {
    json!([
        {
            "name": "list_projects",
            "description": "List Dicta projects from the local filesystem catalog and identify the project for a Git working copy when determinable.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" }
                },
                "additionalProperties": false
            },
            "annotations": { "title": "List Dicta projects", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_current_project",
            "description": "Resolve the Dicta project linked to a Git working copy without contacting the Dicta application or daemon.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" }
                },
                "additionalProperties": false
            },
            "annotations": { "title": "Get the current Dicta project", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_project_guidance",
            "description": "Get the most relevant Dicta guidance for a Git project. Repository-wide recordings and recordings for the selected branch are included.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 25, "default": 8 }
                },
                "additionalProperties": false
            },
            "annotations": { "title": "Get Dicta project guidance", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "list_recordings",
            "description": "List repository-wide and branch-specific Dicta recordings for a Git project in newest-first order.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 }
                },
                "additionalProperties": false
            },
            "annotations": { "title": "List Dicta recordings", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_recording",
            "description": "Read one Dicta recording's complete note, metadata, transcript, and evidence paths.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "recording_id": { "type": "string" }
                },
                "required": ["recording_id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Read a Dicta recording", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_recording_context",
            "description": "Build the concise context instruction for one Dicta recording, including its project and branch or repository scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "recording_id": { "type": "string" }
                },
                "required": ["recording_id"],
                "additionalProperties": false
            },
            "annotations": { "title": "Build Dicta recording context", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        },
        {
            "name": "get_recording_frames",
            "description": "Extract timestamped screenshots from one Dicta recording and return them inline as visual evidence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "branch": { "type": "string" },
                    "recording_id": { "type": "string" },
                    "timestamps_seconds": { "type": "array", "items": { "type": "number", "minimum": 0 }, "maxItems": 8 },
                    "transcript_query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 8, "default": 4 }
                },
                "required": ["recording_id"],
                "additionalProperties": false
            },
            "annotations": { "title": "View Dicta recording frames", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
        }
    ])
}
