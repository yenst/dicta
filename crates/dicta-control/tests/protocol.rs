use dicta_control::{
    codec::{read_frame, write_frame, CodecError},
    error::{ErrorCode, ExitCode},
    protocol::{AppPhase, StatusSnapshot},
    AnnotationTool, Command, Event, EventEnvelope, ModelInstallStage, ModelState,
    ModelStatusSummary, ModelTier, RequestEnvelope, RequestId, Response, ResponseEnvelope,
    ServerMessage,
};
use std::{
    io::{BufReader, Cursor},
    num::NonZeroU64,
};

fn request_id(value: u64) -> RequestId {
    RequestId::new(NonZeroU64::new(value).unwrap())
}

#[test]
fn command_roundtrips_with_a_stable_envelope_shape() {
    let request = RequestEnvelope::new(
        request_id(42),
        Command::RecordStart {
            project: Some("dicta".to_string()),
            note: Some("explain the capture bug".to_string()),
        },
    );
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["id"], 42);
    assert_eq!(json["command"], "record_start");
    assert_eq!(json["params"]["project"], "dicta");
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(json).unwrap(),
        request
    );
}

#[test]
fn model_commands_and_status_have_a_stable_typed_wire_shape() {
    let install = RequestEnvelope::new(
        request_id(43),
        Command::ModelInstall {
            model: ModelTier::Quality,
        },
    );
    let encoded = serde_json::to_value(&install).unwrap();
    assert_eq!(encoded["command"], "model_install");
    assert_eq!(encoded["params"]["model"], "quality");
    assert_eq!(
        serde_json::from_value::<RequestEnvelope>(encoded).unwrap(),
        install
    );

    let status = ResponseEnvelope::success(
        request_id(43),
        Response::ModelStatus(ModelStatusSummary {
            active_model: Some("compact".to_owned()),
            active_model_path: Some("/models/compact.bin".to_owned()),
            quality_state: ModelState::Installing,
            quality_path: "/models/quality.bin".to_owned(),
            quality_size_bytes: 123,
            expected_download_bytes: 456,
            install_stage: Some(ModelInstallStage::Downloading),
            downloaded_bytes: Some(123),
            message: "downloading".to_owned(),
            last_error: None,
        }),
    );
    let encoded = serde_json::to_value(&status).unwrap();
    assert_eq!(encoded["result"]["type"], "model_status");
    assert_eq!(encoded["result"]["data"]["quality_state"], "installing");
    assert_eq!(encoded["result"]["data"]["install_stage"], "downloading");
    assert_eq!(
        serde_json::from_value::<ResponseEnvelope>(encoded).unwrap(),
        status
    );
}

#[test]
fn settings_reuse_the_legacy_document_shape_on_the_wire() {
    let response = ResponseEnvelope::success(
        request_id(44),
        Response::Settings(dicta_core::storage::AppSettings {
            shortcut_id: "control_space".to_owned(),
            cleanup_merged_videos: false,
            branch_locking: true,
            transcription_language: "nl".to_owned(),
            general_path: Some("/data/general".to_owned()),
        }),
    );
    let encoded = serde_json::to_value(&response).unwrap();
    assert_eq!(encoded["result"]["type"], "settings");
    assert_eq!(encoded["result"]["data"]["shortcut_id"], "control_space");
    assert_eq!(
        serde_json::from_value::<ResponseEnvelope>(encoded).unwrap(),
        response
    );
}

#[test]
fn cleanup_summary_preserves_the_legacy_result_shape() {
    let response = ResponseEnvelope::success(
        request_id(45),
        Response::Cleanup(dicta_control::CleanupSummary {
            removed_files: 2,
            freed_bytes: 2048,
            cleaned_branches: vec!["feature/done".to_owned()],
            default_branch: Some("main".to_owned()),
            message: "Removed 2 merged videos.".to_owned(),
        }),
    );
    let encoded = serde_json::to_value(&response).unwrap();
    assert_eq!(encoded["result"]["type"], "cleanup");
    assert_eq!(encoded["result"]["data"]["removed_files"], 2);
    assert_eq!(
        serde_json::from_value::<ResponseEnvelope>(encoded).unwrap(),
        response
    );
}

#[test]
fn malformed_and_empty_frames_are_rejected() {
    let mut malformed = BufReader::new(Cursor::new(b"{nope}\n"));
    assert!(matches!(
        read_frame::<_, RequestEnvelope>(&mut malformed),
        Err(CodecError::Json(_))
    ));

    let mut empty = BufReader::new(Cursor::new(b"\n"));
    assert!(matches!(
        read_frame::<_, RequestEnvelope>(&mut empty),
        Err(CodecError::EmptyFrame)
    ));
}

#[test]
fn oversized_frames_are_rejected_before_deserialization() {
    let mut reader = BufReader::new(Cursor::new(b"123456\n"));
    assert!(matches!(
        dicta_control::codec::read_frame_with_limit::<_, serde_json::Value>(&mut reader, 4),
        Err(CodecError::FrameTooLarge { limit: 4 })
    ));
}

#[test]
fn responses_preserve_request_correlation() {
    let response = ResponseEnvelope::success(request_id(27), Response::Accepted);
    let json = serde_json::to_string(&response).unwrap();
    let decoded: ResponseEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.id, request_id(27));
}

#[test]
fn recording_details_reuse_the_versioned_core_model() {
    let recording: dicta_core::RecordingFile = serde_json::from_value(serde_json::json!({
        "id": "recording-27",
        "project_id": "dicta",
        "note": "Keep the transcript fields",
        "transcript": "Full persisted details survive the wire.",
        "transcription_status": "complete"
    }))
    .unwrap();
    let response = ResponseEnvelope::success(
        request_id(27),
        Response::RecordingDetails(Box::new(recording.clone())),
    );
    let encoded = serde_json::to_value(&response).unwrap();
    assert_eq!(encoded["result"]["type"], "recording_details");
    assert_eq!(encoded["result"]["data"]["id"], "recording-27");
    let decoded: ResponseEnvelope = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        decoded,
        ResponseEnvelope::success(
            request_id(27),
            Response::RecordingDetails(Box::new(recording))
        )
    );
}

#[test]
fn timeline_note_replacement_has_a_stable_typed_wire_shape() {
    let encoded = serde_json::json!({
        "version": 1,
        "id": 28,
        "command": "recording_set_timeline_notes",
        "params": {
            "recording": {"kind": "id", "value": "recording-27"},
            "notes": [{
                "id": "note-1",
                "timestamp_seconds": 12.5,
                "text": "Review this transition",
                "created_at": "2026-08-20T20:18:00Z",
                "source": "typed"
            }]
        }
    });
    let request: RequestEnvelope = serde_json::from_value(encoded.clone()).unwrap();
    let Command::RecordingSetTimelineNotes { recording, notes } = &request.command else {
        panic!("timeline-note command decoded as another variant");
    };
    assert_eq!(
        recording,
        &dicta_control::RecordingSelector::Id("recording-27".to_owned())
    );
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].timestamp_seconds, 12.5);
    assert_eq!(serde_json::to_value(request).unwrap(), encoded);
}

#[test]
fn protocol_version_and_exit_codes_are_stable() {
    let mut request = RequestEnvelope::new(request_id(1), Command::Status);
    request.version = 99;
    let error = request.validate_version().unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedVersion);
    assert_eq!(error.exit_code(), ExitCode::Usage);

    assert_eq!(ErrorCode::NotFound.exit_code().get(), 66);
    assert_eq!(ErrorCode::Unavailable.exit_code().get(), 69);
    assert_eq!(ErrorCode::PermissionDenied.exit_code().get(), 77);
    assert_eq!(ErrorCode::Conflict.exit_code().get(), 78);
}

#[test]
fn events_and_responses_are_independent_ndjson_frames() {
    let event = ServerMessage::Event(EventEnvelope::new(Event::RecordingStarted {
        sequence: 4,
        recording_id: "rec-4".to_string(),
    }));
    let response = ServerMessage::Response(ResponseEnvelope::success(
        request_id(9),
        Response::Status(StatusSnapshot {
            phase: AppPhase::Recording,
            project: Some("dicta".to_string()),
            recording_id: Some("rec-4".to_string()),
            annotations_enabled: true,
            annotation_tool: Some(AnnotationTool::Arrow),
        }),
    ));

    let mut bytes = Vec::new();
    write_frame(&mut bytes, &event).unwrap();
    write_frame(&mut bytes, &response).unwrap();
    assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 2);

    let mut reader = BufReader::new(Cursor::new(bytes));
    assert_eq!(read_frame(&mut reader).unwrap(), Some(event));
    assert_eq!(read_frame(&mut reader).unwrap(), Some(response));
    assert_eq!(read_frame::<_, ServerMessage>(&mut reader).unwrap(), None);
}

#[test]
fn exact_recording_navigation_event_has_a_stable_wire_shape() {
    let event = EventEnvelope::new(Event::UiRecordingRequested {
        sequence: 42,
        recording_id: "take-one".to_owned(),
    });
    let encoded = serde_json::to_value(event).unwrap();
    assert_eq!(encoded["event"], "ui_recording_requested");
    assert_eq!(encoded["data"]["sequence"], 42);
    assert_eq!(encoded["data"]["recording_id"], "take-one");
}
