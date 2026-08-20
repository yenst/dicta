use crate::storage::Recording;
use dicta_core::transcript::format_timestamp;
use serde_json::{json, Value};

pub(crate) fn append_recording_summary(output: &mut String, recording: &Recording) {
    output.push_str(&format!("\n- **{}**\n", display_note(recording)));
    if !recording.note.trim().is_empty() {
        output.push_str(&format!("  - Note: {}\n", recording.note.trim()));
    }
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
    append_timeline_notes(output, recording, true);
    if !recording.transcript_segments.is_empty() {
        output.push_str("  - Timestamped transcript highlights:\n");
        for segment in recording.transcript_segments.iter().take(4) {
            output.push_str(&format!(
                "    - `[{}–{}]` {}\n",
                format_timestamp(segment.start_seconds),
                format_timestamp(segment.end_seconds),
                segment.text
            ));
        }
        if recording.transcript_segments.len() > 4 {
            output.push_str("    - …call get_recording for the complete timestamped transcript.\n");
        }
    } else if let Some(transcript) = recording.transcript.as_deref() {
        let preview = transcript
            .split_whitespace()
            .take(80)
            .collect::<Vec<_>>()
            .join(" ");
        let suffix = if transcript.split_whitespace().count() > 80 {
            "…"
        } else {
            ""
        };
        output.push_str(&format!("  - Transcript preview: {preview}{suffix}\n"));
    } else {
        output.push_str("  - Transcript: not available\n");
    }
}

pub(crate) fn append_transcript(output: &mut String, recording: &Recording) {
    if !recording.transcript_segments.is_empty() {
        output.push_str("\n## Timestamped transcript\n");
        for segment in &recording.transcript_segments {
            output.push_str(&format!(
                "\n- `[{}–{}]` {}\n",
                format_timestamp(segment.start_seconds),
                format_timestamp(segment.end_seconds),
                segment.text
            ));
        }
        output.push_str("\nUse these timestamps with get_recording_frames, or pass transcript_query to resolve matching frames automatically.\n");
    } else if let Some(transcript) = recording.transcript.as_deref() {
        output.push_str(&format!("\n## Transcript\n\n{transcript}\n\nThis legacy transcript has no segment timestamps. Retranscribe it in Dicta for exact frame matching.\n"));
    } else {
        output.push_str("\nNo transcript is available yet. The note and video path are the available evidence.\n");
    }
}

pub(crate) fn recording_json(recording: &Recording) -> Value {
    json!({
        "id": recording.id,
        "note": recording.note,
        "started_at": recording.started_at,
        "duration_seconds": recording.duration_seconds,
        "video_path": recording.video_path,
        "metadata_path": recording.metadata_path,
        "transcript": recording.transcript,
        "transcript_segments": recording.transcript_segments,
        "timeline_notes": recording.metadata.get("timeline_notes").cloned().unwrap_or_else(|| json!([])),
        "metadata": recording.metadata
    })
}

pub(crate) fn timeline_notes(recording: &Recording) -> impl Iterator<Item = &Value> {
    recording
        .metadata
        .get("timeline_notes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

pub(crate) fn append_timeline_notes(output: &mut String, recording: &Recording, compact: bool) {
    let notes = timeline_notes(recording).collect::<Vec<_>>();
    if notes.is_empty() {
        return;
    }
    if compact {
        output.push_str(&format!("  - Timeline notes: {}\n", notes.len()));
        return;
    }
    output.push_str("\n## Timeline notes\n");
    for note in notes {
        let seconds = note
            .get("timestamp_seconds")
            .and_then(Value::as_f64)
            .unwrap_or_default()
            .max(0.0)
            .floor() as u64;
        let text = note
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("Untitled note")
            .replace(['\r', '\n'], " ");
        output.push_str(&format!(
            "\n- `{:02}:{:02}` — {}\n",
            seconds / 60,
            seconds % 60,
            text
        ));
    }
}

pub(crate) fn append_warnings(output: &mut String, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    output.push_str("\n## Storage warnings\n");
    for warning in warnings {
        output.push_str(&format!("\n- {warning}\n"));
    }
}

pub(crate) fn display_note(recording: &Recording) -> &str {
    let note = recording.note.trim();
    if note.is_empty() {
        recording.id.as_str()
    } else {
        note
    }
}
