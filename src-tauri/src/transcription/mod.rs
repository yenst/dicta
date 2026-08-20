mod model;
pub(crate) use model::*;

use crate::*;

pub(crate) fn write_recording(recording: &Recording) -> Result<(), String> {
    storage::recordings::write(Path::new(&recording.metadata_path), recording)
}

pub(crate) fn clean_segment_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn normalize_transcript_segments(
    segments: &[TranscriptSegment],
) -> Vec<TranscriptSegment> {
    let mut normalized = segments
        .iter()
        .filter_map(|segment| {
            let text = clean_segment_text(&segment.text);
            if text.is_empty()
                || !segment.start_seconds.is_finite()
                || !segment.end_seconds.is_finite()
                || segment.start_seconds < 0.0
            {
                return None;
            }
            Some(TranscriptSegment {
                start_seconds: segment.start_seconds,
                end_seconds: segment.end_seconds.max(segment.start_seconds),
                text,
            })
        })
        .take(10_000)
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| {
        left.start_seconds
            .partial_cmp(&right.start_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut grouped: Vec<TranscriptSegment> = Vec::new();
    for segment in normalized {
        let should_join = grouped.last().is_some_and(|current| {
            let gap = segment.start_seconds - current.end_seconds;
            let current_duration = current.end_seconds - current.start_seconds;
            gap <= 1.0 && current_duration < 6.0 && !current.text.ends_with(['.', '?', '!'])
        });
        if should_join {
            let current = grouped.last_mut().expect("checked above");
            current.text.push(' ');
            current.text.push_str(&segment.text);
            current.end_seconds = current.end_seconds.max(segment.end_seconds);
        } else {
            grouped.push(segment);
        }
    }
    grouped
}

pub(crate) fn timestamped_transcript(transcript: &str, segments: &[TranscriptSegment]) -> String {
    if segments.is_empty() {
        return transcript.trim().to_string();
    }
    segments
        .iter()
        .map(|segment| {
            format!(
                "[{}–{}] {}",
                transcript_timestamp(segment.start_seconds),
                transcript_timestamp(segment.end_seconds),
                segment.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn update_transcription(
    payload: &NativeTranscriptionPayload,
) -> Result<Recording, String> {
    let metadata_path = PathBuf::from(&payload.path).with_extension("json");
    let transcript = payload.transcript.clone();
    let error = payload.error.clone();
    let transcript_segments = normalize_transcript_segments(&payload.transcript_segments);
    let (recording, ()) =
        storage::recordings::update::<Recording, _>(&metadata_path, |recording| {
            if let Some(transcript) = transcript.as_deref() {
                let transcript_path = metadata_path.with_extension("transcript.md");
                fs::write(
                    &transcript_path,
                    format!(
                        "{}\n",
                        timestamped_transcript(transcript, &transcript_segments)
                    ),
                )
                .map_err(|error| format!("Could not write transcript: {error}"))?;
                let transcript_json_path = metadata_path.with_extension("transcript.json");
                let transcript_json = serde_json::to_string_pretty(&serde_json::json!({
                    "version": 1,
                    "transcript": transcript,
                    "transcript_segments": &transcript_segments,
                }))
                .map_err(|error| format!("Could not encode timed transcript: {error}"))?;
                fs::write(&transcript_json_path, format!("{transcript_json}\n"))
                    .map_err(|error| format!("Could not write timed transcript: {error}"))?;
                recording.transcript = Some(transcript.to_string());
                recording.transcript_path = Some(path_string(&transcript_path));
                recording.transcript_segments = transcript_segments;
                recording.transcription_status = TranscriptionStatus::Complete;
                recording.transcription_error = None;
            } else {
                recording.transcription_status = TranscriptionStatus::Failed;
                recording.transcription_error = error;
            }
            Ok(())
        })?;
    Ok(recording)
}

pub(crate) fn mark_transcription_processing(
    video_path: &str,
    language: &str,
) -> Result<(), String> {
    let metadata_path = PathBuf::from(video_path).with_extension("json");
    storage::recordings::update::<Recording, _>(&metadata_path, |recording| {
        recording.transcription_status = TranscriptionStatus::Processing;
        recording.transcription_error = None;
        recording.transcription_language = Some(language.to_string());
        Ok(())
    })
    .map(|_| ())
}

pub(crate) fn whisper_prompt(language: &str) -> &'static str {
    if language == "nl" {
        "Nederlandse technische uitleg over softwareontwikkeling, API-integraties, broncode en implementatiedetails."
    } else {
        "Technical software explanation about APIs, source code, and implementation details."
    }
}

pub(crate) fn loaded_whisper(
    app: &AppHandle,
) -> Result<std::sync::MutexGuard<'static, Option<LoadedWhisper>>, String> {
    let model_path = selected_whisper_model(app)?;
    let mut slot = WHISPER_MODEL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let needs_load = slot
        .as_ref()
        .map(|loaded| loaded.path != model_path)
        .unwrap_or(true);
    if needs_load {
        let context =
            WhisperContext::new_with_params(&model_path, WhisperContextParameters::default())
                .map_err(|error| format!("Could not load Dicta's Whisper model: {error}"))?;
        *slot = Some(LoadedWhisper {
            path: model_path,
            context,
        });
    }
    Ok(slot)
}

pub(crate) fn local_whisper_transcript(
    app: &AppHandle,
    video_path: &str,
    language: &str,
) -> Result<LocalTranscript, String> {
    let mut hasher = DefaultHasher::new();
    video_path.hash(&mut hasher);
    let wav_path = std::env::temp_dir().join(format!("dicta-{}.wav", hasher.finish()));
    let extracted = platform::extract_audio(video_path, &path_string(&wav_path));
    if !extracted {
        return Err("Dicta could not extract narration from the recording".to_string());
    }

    let result = (|| {
        let mut reader = hound::WavReader::open(&wav_path)
            .map_err(|error| format!("Could not read extracted narration: {error}"))?;
        let spec = reader.spec();
        if spec.channels != 1 || spec.sample_rate != 16_000 {
            return Err("Extracted narration was not 16 kHz mono audio".to_string());
        }
        let samples = reader
            .samples::<i16>()
            .map(|sample| {
                sample
                    .map(|value| value as f32 / i16::MAX as f32)
                    .map_err(|error| format!("Invalid narration sample: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if samples.is_empty() {
            return Err("No narration audio was found in this recording".to_string());
        }

        let loaded = loaded_whisper(app)?;
        let context = loaded
            .as_ref()
            .ok_or_else(|| "Dicta's Whisper model failed to load".to_string())?;
        let mut state = context
            .context
            .create_state()
            .map_err(|error| format!("Could not start local transcription: {error}"))?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: -1.0,
        });
        params.set_language(if language == "auto" {
            None
        } else {
            Some(language)
        });
        params.set_initial_prompt(whisper_prompt(language));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        state
            .full(params, &samples)
            .map_err(|error| format!("Local transcription failed: {error}"))?;
        let segments = state
            .as_iter()
            .filter_map(|segment| {
                let text = clean_segment_text(&segment.to_string());
                if text.is_empty() {
                    return None;
                }
                Some(TranscriptSegment {
                    // whisper.cpp exposes segment timestamps in centiseconds.
                    start_seconds: segment.start_timestamp() as f64 / 100.0,
                    end_seconds: segment.end_timestamp() as f64 / 100.0,
                    text,
                })
            })
            .collect::<Vec<_>>();
        let segments = normalize_transcript_segments(&segments);
        let transcript = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if transcript.is_empty() {
            Err("No speech was detected in this recording".to_string())
        } else {
            Ok(LocalTranscript {
                transcript,
                segments,
            })
        }
    })();
    let _ = fs::remove_file(wav_path);
    result
}

#[tauri::command]
pub(crate) async fn transcribe_voice_note(
    app: AppHandle,
    audio_bytes: Vec<u8>,
    mime_type: String,
    language: String,
) -> Result<String, String> {
    if !is_allowed_language(&language) {
        return Err(format!("Unsupported transcription language: {language}"));
    }
    if audio_bytes.len() < 128 {
        return Err("The voice note did not contain enough audio".to_string());
    }
    if audio_bytes.len() > 16 * 1024 * 1024 {
        return Err("Voice notes must be shorter than 16 MB".to_string());
    }
    let normalized_mime = mime_type.split(';').next().unwrap_or("");
    let extension = match normalized_mime {
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => return Err("Dicta does not support this microphone audio format".to_string()),
    };
    let mut hasher = DefaultHasher::new();
    audio_bytes.hash(&mut hasher);
    let audio_path = std::env::temp_dir().join(format!(
        "dicta-voice-{}-{}.{}",
        Utc::now().timestamp_millis(),
        hasher.finish(),
        extension
    ));
    fs::write(&audio_path, &audio_bytes)
        .map_err(|error| format!("Could not prepare the voice note: {error}"))?;
    let audio_path_string = path_string(&audio_path);
    let app_for_transcription = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let lock = LOCAL_TRANSCRIBER.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        local_whisper_transcript(&app_for_transcription, &audio_path_string, &language)
            .map(|result| result.transcript)
    })
    .await;
    let _ = fs::remove_file(audio_path);
    joined.map_err(|error| format!("Voice transcription stopped unexpectedly: {error}"))?
}

pub(crate) fn queue_local_transcription(app: &AppHandle, video_path: String, language: String) {
    let _ = mark_transcription_processing(&video_path, &language);
    let app = app.clone();
    std::thread::spawn(move || {
        let lock = LOCAL_TRANSCRIBER.get_or_init(|| Mutex::new(()));
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = local_whisper_transcript(&app, &video_path, &language);
        let payload = match result {
            Ok(result) => NativeTranscriptionPayload {
                path: video_path,
                transcript: Some(result.transcript),
                transcript_segments: result.segments,
                error: None,
            },
            Err(error) => NativeTranscriptionPayload {
                path: video_path,
                transcript: None,
                transcript_segments: Vec::new(),
                error: Some(error),
            },
        };
        let updated = update_transcription(&payload);
        let state = app.state::<AppState>();
        let status = state
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .status
            .clone();
        match updated {
            Ok(recording) if recording.transcription_status == TranscriptionStatus::Complete => {
                emit_recorder_event(&app, "transcribed", "Transcript ready for agents", status)
            }
            Ok(recording) => emit_recorder_event(
                &app,
                "transcription_error",
                recording
                    .transcription_error
                    .as_deref()
                    .unwrap_or("Local transcription failed"),
                status,
            ),
            Err(error) => emit_recorder_event(&app, "transcription_error", &error, status),
        }
    });
}

#[tauri::command]
pub(crate) fn retranscribe_recording(
    app: AppHandle,
    project_id: String,
    recording_id: String,
    language: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !is_allowed_language(&language) {
        return Err(format!("Unsupported transcription language: {language}"));
    }
    let recording = load_recordings(&state.root, &project_id)?
        .into_iter()
        .find(|recording| recording.id.as_str() == recording_id)
        .ok_or_else(|| format!("Recording not found: {recording_id}"))?;
    if !recording.success || !Path::new(&recording.video_path).exists() {
        return Err("This recording has no usable video to transcribe".to_string());
    }
    mark_transcription_processing(&recording.video_path, &language)?;
    emit_recorder_event(
        &app,
        "transcribing",
        "Transcribing with the selected language…",
        state
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .status
            .clone(),
    );
    queue_local_transcription(&app, recording.video_path, language);
    Ok(())
}

pub(crate) fn queue_transcription(video_path: &str, language: &str) -> Result<(), String> {
    platform::transcribe(video_path, language, native_recorder_callback)
}

pub(crate) fn should_retry_transcription(recording: &Recording) -> bool {
    if !recording.success || !Path::new(&recording.video_path).exists() {
        return false;
    }
    if !recording
        .transcript
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return false;
    }
    matches!(
        recording.transcription_status.as_str(),
        "pending" | "processing" | ""
    )
}

pub(crate) fn language_for_recording(root: &Path, recording: &Recording) -> String {
    recording
        .transcription_language
        .as_deref()
        .filter(|language| is_allowed_language(language))
        .map(str::to_string)
        .unwrap_or_else(|| settings_language(root))
}

pub(crate) fn queue_pending_transcriptions(root: &Path) {
    for project in load_projects(root) {
        let Ok(recordings) = load_recordings(root, &project.id) else {
            continue;
        };
        for recording in recordings {
            if should_retry_transcription(&recording) {
                let language = language_for_recording(root, &recording);
                let _ = queue_transcription(&recording.video_path, &language);
            }
        }
    }
}

pub(crate) fn poster_path_for_video(video_path: &str) -> PathBuf {
    PathBuf::from(video_path).with_extension("poster.jpg")
}

pub(crate) fn extract_poster(video_path: &str) -> Option<String> {
    let poster = poster_path_for_video(video_path);
    if fs::symlink_metadata(&poster)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        return Some(path_string(&poster));
    }
    if !Path::new(video_path).is_file() {
        return None;
    }
    if platform::extract_poster(video_path, &path_string(&poster))
        && fs::symlink_metadata(&poster)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        return Some(path_string(&poster));
    }
    None
}

pub(crate) fn attach_poster(recording: Recording) -> Result<Recording, String> {
    if recording
        .poster_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file())
    {
        return Ok(recording);
    }
    let Some(poster) = extract_poster(&recording.video_path) else {
        return Ok(recording);
    };
    let metadata_path = PathBuf::from(&recording.metadata_path);
    storage::recordings::update::<Recording, _>(&metadata_path, |current| {
        current.poster_path = Some(poster);
        Ok(())
    })
    .map(|(recording, ())| recording)
}
