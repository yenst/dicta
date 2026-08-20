use crate::tools::{self, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

const SERVER_NAME: &str = "dicta";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const SUPPORTED_PROTOCOLS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

#[derive(Deserialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
}

#[derive(Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default = "empty_object")]
    arguments: Value,
}

fn empty_object() -> Value {
    json!({})
}

pub fn process_line(line: &str) -> Option<Value> {
    if line.trim().is_empty() {
        return None;
    }
    let request = match serde_json::from_str::<Value>(line) {
        Ok(request) => request,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                "Parse error",
                Some(json!({ "detail": error.to_string() })),
            ));
        }
    };
    process_request(request)
}

pub fn process_request(request: Value) -> Option<Value> {
    let Some(object) = request.as_object() else {
        return Some(error_response(Value::Null, -32600, "Invalid Request", None));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_response(
            valid_id(object.get("id")).unwrap_or(Value::Null),
            -32600,
            "Invalid Request",
            None,
        ));
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Some(error_response(
            valid_id(object.get("id")).unwrap_or(Value::Null),
            -32600,
            "Invalid Request",
            None,
        ));
    };
    let value = object.get("id")?;
    let id = match valid_id(Some(value)) {
        Some(id) => id,
        None => return Some(error_response(Value::Null, -32600, "Invalid Request", None)),
    };
    let params = object.get("params").cloned().unwrap_or_else(empty_object);
    Some(handle_method(method, params, id))
}

fn valid_id(id: Option<&Value>) -> Option<Value> {
    let id = id?;
    (id.is_null() || id.is_string() || id.is_number()).then(|| id.clone())
}

fn handle_method(method: &str, params: Value, id: Value) -> Value {
    match method {
        "initialize" => initialize(id, params),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tools::definitions() })),
        "resources/list" => success(id, json!({ "resources": [] })),
        "prompts/list" => success(id, json!({ "prompts": [] })),
        "tools/call" => call_tool(id, params),
        _ => error_response(
            id,
            -32601,
            "Method not found",
            Some(json!({ "method": method })),
        ),
    }
}

fn initialize(id: Value, params: Value) -> Value {
    let params = match serde_json::from_value::<InitializeParams>(params) {
        Ok(params) => params,
        Err(error) => return invalid_params(id, error.to_string()),
    };
    if !SUPPORTED_PROTOCOLS.contains(&params.protocol_version.as_str()) {
        return error_response(
            id,
            -32602,
            "Unsupported protocol version",
            Some(json!({ "requested": params.protocol_version, "supported": SUPPORTED_PROTOCOLS })),
        );
    }
    success(
        id,
        json!({
            "protocolVersion": params.protocol_version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
            "instructions": "Dicta contains screen-and-voice guidance recorded for Git projects. Recordings may apply to the whole repository or only one branch. Use list_projects or get_current_project when project identity is needed. When the user asks to check Dicta, prior recordings, project guidance, or previously explained context, call get_project_guidance with the current workspace path. Use the current Git branch unless the user names another branch; repository-wide recordings are included automatically. Treat results as supporting context and inspect referenced repository files before changing code. Timestamped transcript segments identify when spoken guidance occurred. When the transcript refers to something visible on screen or visual evidence is needed, call get_recording_frames with explicit timestamps or transcript_query. All Dicta tools are read-only and work without the Dicta daemon."
        }),
    )
}

fn call_tool(id: Value, params: Value) -> Value {
    let params = match serde_json::from_value::<ToolCallParams>(params) {
        Ok(params) if params.arguments.is_object() => params,
        Ok(_) => return invalid_params(id, "arguments must be an object".to_string()),
        Err(error) => return invalid_params(id, error.to_string()),
    };
    match tools::call(&params.name, params.arguments) {
        Ok(ToolResult::Text { text, structured }) => success(
            id,
            json!({ "content": [{ "type": "text", "text": text }], "structuredContent": structured, "isError": false }),
        ),
        Ok(ToolResult::Images {
            text,
            structured,
            images,
        }) => {
            let mut content = vec![json!({ "type": "text", "text": text })];
            content.extend(images.into_iter().map(|image| {
                json!({ "type": "image", "data": base64_encode(&image), "mimeType": "image/jpeg" })
            }));
            success(
                id,
                json!({ "content": content, "structuredContent": structured, "isError": false }),
            )
        }
        Err(message) => success(
            id,
            json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
        ),
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn invalid_params(id: Value, detail: String) -> Value {
    error_response(
        id,
        -32602,
        "Invalid params",
        Some(json!({ "detail": detail })),
    )
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_json_returns_parse_error() {
        let response = process_line("{").unwrap();
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32700);
    }

    #[test]
    fn structurally_invalid_request_returns_invalid_request() {
        let response = process_line(r#"{"jsonrpc":"2.0","id":1}"#).unwrap();
        assert_eq!(response["error"]["code"], -32600);
    }

    #[test]
    fn notifications_do_not_get_responses() {
        assert!(
            process_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none()
        );
    }

    #[test]
    fn unsupported_protocol_is_rejected() {
        let response = process_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#).unwrap();
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn known_protocol_initializes() {
        let response = process_line(r#"{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#).unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(response["result"]["serverInfo"]["name"], "dicta");
    }

    #[test]
    fn typed_tool_arguments_reject_unknown_fields() {
        let response = process_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_recordings","arguments":{"unexpected":true}}}"#).unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown field"));
    }

    #[test]
    fn encodes_frame_bytes_as_base64() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }
}
