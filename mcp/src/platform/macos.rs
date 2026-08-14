use std::{ffi::CString, os::raw::c_char, path::Path};

unsafe extern "C" {
    fn dicta_mcp_extract_frame(
        input_path: *const c_char,
        requested_seconds: f64,
        output_path: *const c_char,
        actual_seconds: *mut f64,
    ) -> bool;
}

pub(crate) fn extract_frame(
    video_path: &Path,
    seconds: f64,
    output_path: &Path,
) -> Result<f64, String> {
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
            crate::format_timestamp(seconds),
            video_path.display()
        ))
    }
}
