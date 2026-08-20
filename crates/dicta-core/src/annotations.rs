use crate::{AnnotationId, RecordingId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const ANNOTATION_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationTool {
    Pen,
    Arrow,
    Rectangle,
    Spotlight,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct NormalizedPoint {
    pub x: f32,
    pub y: f32,
}

impl NormalizedPoint {
    pub fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && (0.0..=1.0).contains(&self.x)
            && (0.0..=1.0).contains(&self.y)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AnnotationStyle {
    pub color: String,
    pub width: f32,
    #[serde(default = "opaque")]
    pub opacity: f32,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl AnnotationStyle {
    pub fn is_valid(&self) -> bool {
        !self.color.trim().is_empty()
            && self.width.is_finite()
            && self.width > 0.0
            && self.opacity.is_finite()
            && (0.0..=1.0).contains(&self.opacity)
    }
}

fn opaque() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AnnotationEvent {
    pub id: AnnotationId,
    pub tool: AnnotationTool,
    pub started_at_seconds: f64,
    #[serde(default)]
    pub ended_at_seconds: Option<f64>,
    pub style: AnnotationStyle,
    #[serde(default)]
    pub points: Vec<NormalizedPoint>,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl AnnotationEvent {
    pub fn is_valid(&self) -> bool {
        self.started_at_seconds.is_finite()
            && self.started_at_seconds >= 0.0
            && self
                .ended_at_seconds
                .is_none_or(|ended| ended.is_finite() && ended >= self.started_at_seconds)
            && self.style.is_valid()
            && !self.points.is_empty()
            && self.points.iter().copied().all(NormalizedPoint::is_valid)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AnnotationCanvas {
    #[serde(default)]
    pub output_name: Option<String>,
    pub width_pixels: u32,
    pub height_pixels: u32,
    #[serde(default = "unit_scale")]
    pub scale: f32,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl AnnotationCanvas {
    pub fn is_valid(&self) -> bool {
        self.width_pixels > 0
            && self.height_pixels > 0
            && self.scale.is_finite()
            && self.scale > 0.0
    }
}

fn unit_scale() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AnnotationFile {
    #[serde(default = "annotation_format_version")]
    pub version: u32,
    pub recording_id: RecordingId,
    pub canvas: AnnotationCanvas,
    #[serde(default)]
    pub events: Vec<AnnotationEvent>,
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
}

impl AnnotationFile {
    pub fn new(recording_id: RecordingId, canvas: AnnotationCanvas) -> Self {
        Self {
            version: ANNOTATION_FORMAT_VERSION,
            recording_id,
            canvas,
            events: Vec::new(),
            extra: Map::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.version > 0
            && self.canvas.is_valid()
            && self.events.iter().all(AnnotationEvent::is_valid)
    }
}

fn annotation_format_version() -> u32 {
    ANNOTATION_FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> AnnotationEvent {
        AnnotationEvent {
            id: AnnotationId::new("stroke-1").unwrap(),
            tool: AnnotationTool::Pen,
            started_at_seconds: 2.5,
            ended_at_seconds: Some(3.0),
            style: AnnotationStyle {
                color: "#f7768e".to_string(),
                width: 4.0,
                opacity: 0.8,
                extra: Map::new(),
            },
            points: vec![
                NormalizedPoint { x: 0.1, y: 0.2 },
                NormalizedPoint { x: 0.3, y: 0.4 },
            ],
            extra: Map::new(),
        }
    }

    #[test]
    fn annotation_files_round_trip_and_preserve_unknown_fields() {
        let value = json!({
            "version": 1,
            "recording_id": "20260820-12-00-00",
            "canvas": {
                "output_name": "DP-1",
                "width_pixels": 2560,
                "height_pixels": 1440,
                "scale": 1.25,
                "future_canvas_field": true
            },
            "events": [{
                "id": "stroke-1",
                "tool": "pen",
                "started_at_seconds": 2.5,
                "ended_at_seconds": 3.0,
                "style": {
                    "color": "#f7768e",
                    "width": 4.0,
                    "opacity": 0.8,
                    "pressure": true
                },
                "points": [{"x": 0.1, "y": 0.2}],
                "future_event_field": "kept"
            }],
            "future_file_field": {"kept": true}
        });

        let annotations: AnnotationFile = serde_json::from_value(value).unwrap();
        assert!(annotations.is_valid());
        let encoded = serde_json::to_value(annotations).unwrap();
        assert_eq!(encoded["future_file_field"]["kept"], true);
        assert_eq!(encoded["canvas"]["future_canvas_field"], true);
        assert_eq!(encoded["events"][0]["future_event_field"], "kept");
        assert_eq!(encoded["events"][0]["style"]["pressure"], true);
    }

    #[test]
    fn invalid_coordinates_and_times_are_rejected() {
        let mut candidate = event();
        assert!(candidate.is_valid());
        candidate.points[0].x = 1.1;
        assert!(!candidate.is_valid());
        candidate.points[0].x = 0.5;
        candidate.ended_at_seconds = Some(1.0);
        assert!(!candidate.is_valid());
    }

    #[test]
    fn missing_version_defaults_to_current_format() {
        let value = json!({
            "recording_id": "20260820-12-00-00",
            "canvas": {"width_pixels": 1920, "height_pixels": 1080}
        });
        let annotations: AnnotationFile = serde_json::from_value(value).unwrap();
        assert_eq!(annotations.version, ANNOTATION_FORMAT_VERSION);
        assert_eq!(annotations.canvas.scale, 1.0);
    }
}
