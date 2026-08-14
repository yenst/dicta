mod capture;
mod media;
mod speech;

pub(crate) use capture::{start_recording, stop_recording};
pub(crate) use media::{extract_audio, extract_poster};
pub(crate) use speech::transcribe;
