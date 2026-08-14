use crate::platform::NativeCallback;
use std::{ffi::CString, os::raw::c_char};

unsafe extern "C" {
    fn dicta_transcribe(
        input_path: *const c_char,
        language: *const c_char,
        callback: NativeCallback,
    );
}

pub(crate) fn transcribe(
    input_path: &str,
    language: &str,
    callback: NativeCallback,
) -> Result<(), String> {
    let input_path = CString::new(input_path)
        .map_err(|_| "The transcription path contains an unsupported character".to_string())?;
    let language = CString::new(language)
        .map_err(|_| "The transcription language contains an unsupported character".to_string())?;
    unsafe { dicta_transcribe(input_path.as_ptr(), language.as_ptr(), callback) };
    Ok(())
}
