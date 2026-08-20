use crate::{context, platform, render, search, storage::Recording};
use dicta_core::transcript::format_timestamp;
use serde::Deserialize;
use serde_json::json;
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FramesArgs {
    repo_path: Option<String>,
    branch: Option<String>,
    recording_id: String,
    timestamps_seconds: Option<Vec<f64>>,
    transcript_query: Option<String>,
    limit: Option<u64>,
}

pub(super) fn get(args: FramesArgs) -> Result<super::ToolResult, String> {
    if args.recording_id.trim().is_empty() {
        return Err("recording_id is required".to_string());
    }
    let context = context::resolve(args.repo_path.as_deref(), args.branch.as_deref())?;
    let recording = context::find(&context, args.recording_id.trim())?.recording;
    if !Path::new(&recording.video_path).is_file() {
        return Err(format!(
            "The recording video is missing at `{}`",
            recording.video_path
        ));
    }
    let timestamps = requested_timestamps(&recording, &args)?;
    let output_dir = create_temp_dir()?;
    let mut text = format!(
        "# Timestamped frames: {}\n\nRecording ID: `{}`\nVideo: `{}`\n",
        render::display_note(&recording),
        recording.id,
        recording.video_path
    );
    if !recording.transcript_segments.is_empty() {
        text.push_str("Transcript excerpts below use stored segment timestamps; the screenshot timestamps are exact.\n");
    } else if recording.transcript.is_some() {
        text.push_str("This legacy transcript has no segment timestamps, so excerpts below are approximate position-based context; the screenshot timestamps are exact.\n");
    }
    let safe_id = safe_file_component(recording.id.as_str());
    let mut images = Vec::new();
    let mut frames = Vec::new();
    for requested_seconds in timestamps {
        let millis = (requested_seconds * 1000.0).round() as u64;
        let output_path = output_dir
            .path()
            .join(format!("{safe_id}-{millis:010}.jpg"));
        let actual_seconds = platform::extract_frame(
            Path::new(&recording.video_path),
            requested_seconds,
            &output_path,
        )?;
        let image = fs::read(&output_path)
            .map_err(|error| format!("Could not read extracted frame: {error}"))?;
        let timestamp = format_timestamp(actual_seconds);
        let excerpt = search::transcript_excerpt(&recording, actual_seconds);
        text.push_str(&format!(
            "\n## {timestamp}\n\nScreenshot returned inline.\n"
        ));
        if let Some(excerpt_text) = excerpt.text.as_deref() {
            let label = if excerpt.timing == "timestamped_segment" {
                "Timestamped transcript context"
            } else {
                "Approximate transcript context"
            };
            text.push_str(&format!("{label}: {excerpt_text}\n"));
        }
        frames.push(json!({
            "timestamp": timestamp,
            "timestamp_seconds": actual_seconds,
            "requested_seconds": requested_seconds,
            "mime_type": "image/jpeg",
            "transcript_excerpt": excerpt.text,
            "transcript_timing": excerpt.timing
        }));
        images.push(image);
    }
    Ok(super::ToolResult::Images {
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

fn requested_timestamps(recording: &Recording, args: &FramesArgs) -> Result<Vec<f64>, String> {
    let duration = recording
        .duration_seconds
        .filter(|value| value.is_finite() && *value > 0.0);
    if let Some(values) = &args.timestamps_seconds {
        if values.is_empty() {
            return Err("timestamps_seconds must contain at least one timestamp".to_string());
        }
        if values.len() > 8 {
            return Err("timestamps_seconds must not contain more than 8 timestamps".to_string());
        }
        let mut timestamps = Vec::new();
        for value in values {
            let timestamp = value;
            if !timestamp.is_finite() || *timestamp < 0.0 {
                return Err(
                    "Every timestamp must be a finite non-negative number of seconds".to_string(),
                );
            }
            let timestamp = duration
                .map(|duration| timestamp.min((duration - 0.05).max(0.0)))
                .unwrap_or(*timestamp);
            if !timestamps
                .iter()
                .any(|existing: &f64| (*existing - timestamp).abs() < 0.01)
            {
                timestamps.push(timestamp);
            }
        }
        return Ok(timestamps);
    }
    if let Some(query) = args
        .transcript_query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        if recording.transcript_segments.is_empty() {
            return Err("This recording has no timestamped transcript segments. Pass timestamps_seconds explicitly or retranscribe it in Dicta.".to_string());
        }
        let limit = super::checked_limit(args.limit, 4, 8)?;
        let timestamps = search::matching_transcript_timestamps(recording, query, limit);
        if timestamps.is_empty() {
            return Err(format!(
                "No timestamped transcript segments matched `{query}`"
            ));
        }
        return Ok(timestamps);
    }
    let duration = duration.ok_or_else(|| {
        "This recording has no duration metadata. Pass timestamps_seconds explicitly.".to_string()
    })?;
    let limit = super::checked_limit(args.limit, 4, 8)?;
    if duration <= 1.0 {
        return Ok(vec![0.0]);
    }
    let count = limit.min((duration / 4.0).ceil() as usize).max(1);
    Ok((1..=count)
        .map(|index| duration * index as f64 / (count + 1) as f64)
        .collect())
}

fn create_temp_dir() -> Result<tempfile::TempDir, String> {
    use std::os::unix::fs::PermissionsExt;
    let mut builder = tempfile::Builder::new();
    builder
        .prefix("dicta-mcp-frames-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .map_err(|error| format!("Could not create the temporary frame folder: {error}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use dicta_core::{RecordingId, TranscriptSegment};
    use std::os::unix::fs::PermissionsExt;

    fn recording(duration_seconds: Option<f64>) -> Recording {
        Recording {
            id: RecordingId::new("one").unwrap(),
            note: String::new(),
            started_at: None,
            duration_seconds,
            video_path: String::new(),
            metadata_path: String::new(),
            transcript: None,
            transcript_segments: Vec::new(),
            metadata: json!({}),
        }
    }

    fn args() -> FramesArgs {
        FramesArgs {
            repo_path: None,
            branch: None,
            recording_id: "one".to_string(),
            timestamps_seconds: None,
            transcript_query: None,
            limit: Some(4),
        }
    }

    #[test]
    fn automatic_frames_are_evenly_spaced() {
        assert_eq!(
            requested_timestamps(&recording(Some(100.0)), &args()).unwrap(),
            vec![20.0, 40.0, 60.0, 80.0]
        );
    }

    #[test]
    fn transcript_queries_use_segment_timestamps() {
        let mut recording = recording(Some(90.0));
        recording.transcript_segments = vec![TranscriptSegment {
            start_seconds: 41.0,
            end_seconds: 47.0,
            text: "Open the retry menu".to_string(),
        }];
        let mut args = args();
        args.transcript_query = Some("retry menu".to_string());
        assert_eq!(requested_timestamps(&recording, &args).unwrap(), vec![44.0]);
    }

    #[test]
    fn frame_temp_directories_are_private_and_owned() {
        let directory = create_temp_dir().unwrap();
        let path = directory.path().to_path_buf();
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        drop(directory);
        assert!(!path.exists());
    }
}
