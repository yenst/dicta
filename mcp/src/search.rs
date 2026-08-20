use crate::{render::timeline_notes, storage::Recording};

pub(crate) fn relevance(recording: &Recording, query: &str) -> usize {
    let timeline_note_text = timeline_notes(recording)
        .filter_map(|note| note.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let haystack = format!(
        "{} {} {}",
        recording.note,
        recording.transcript.as_deref().unwrap_or_default(),
        timeline_note_text
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

fn normalized_terms(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() > 1)
        .map(str::to_string)
        .collect()
}

pub(crate) fn matching_transcript_timestamps(
    recording: &Recording,
    query: &str,
    limit: usize,
) -> Vec<f64> {
    let normalized_query = query.trim().to_lowercase();
    let query_terms = normalized_terms(query);
    let mut matches = recording
        .transcript_segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.to_lowercase();
            let overlap = query_terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count();
            let score = if !normalized_query.is_empty() && text.contains(&normalized_query) {
                10_000 + normalized_query.len()
            } else {
                overlap
            };
            (score > 0).then_some((score, segment))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|(left_score, left), (right_score, right)| {
        right_score.cmp(left_score).then_with(|| {
            left.start_seconds
                .partial_cmp(&right.start_seconds)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    matches
        .into_iter()
        .take(limit)
        .map(|(_, segment)| (segment.start_seconds + segment.end_seconds) / 2.0)
        .collect()
}

pub(crate) struct TranscriptExcerpt {
    pub(crate) text: Option<String>,
    pub(crate) timing: &'static str,
}

pub(crate) fn transcript_excerpt(recording: &Recording, seconds: f64) -> TranscriptExcerpt {
    if !recording.transcript_segments.is_empty() {
        let nearest = recording
            .transcript_segments
            .iter()
            .filter(|segment| {
                seconds >= segment.start_seconds - 1.5 && seconds <= segment.end_seconds + 1.5
            })
            .min_by(|left, right| {
                segment_distance(left, seconds)
                    .partial_cmp(&segment_distance(right, seconds))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        return TranscriptExcerpt {
            text: nearest.map(|segment| {
                format!(
                    "[{}–{}] {}",
                    dicta_core::transcript::format_timestamp(segment.start_seconds),
                    dicta_core::transcript::format_timestamp(segment.end_seconds),
                    segment.text
                )
            }),
            timing: "timestamped_segment",
        };
    }
    let Some(transcript) = recording.transcript.as_deref() else {
        return TranscriptExcerpt {
            text: None,
            timing: "unavailable",
        };
    };
    let Some(duration) = recording.duration_seconds.filter(|value| *value > 0.0) else {
        return TranscriptExcerpt {
            text: None,
            timing: "unavailable",
        };
    };
    let words = transcript.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return TranscriptExcerpt {
            text: None,
            timing: "unavailable",
        };
    }
    let center = ((seconds / duration).clamp(0.0, 1.0) * words.len() as f64) as usize;
    let start = center.saturating_sub(24);
    let end = (center + 25).min(words.len());
    let mut excerpt = words[start..end].join(" ");
    if start > 0 {
        excerpt.insert(0, '…');
    }
    if end < words.len() {
        excerpt.push('…');
    }
    TranscriptExcerpt {
        text: Some(excerpt),
        timing: "approximate_position",
    }
}

fn segment_distance(segment: &dicta_core::TranscriptSegment, seconds: f64) -> f64 {
    if seconds < segment.start_seconds {
        segment.start_seconds - seconds
    } else if seconds > segment.end_seconds {
        seconds - segment.end_seconds
    } else {
        0.0
    }
}
