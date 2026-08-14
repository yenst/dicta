use crate::platform::NativeCallback;
use std::{ffi::CString, os::raw::c_char};

unsafe extern "C" {
    fn dicta_start(output_path: *const c_char, callback: NativeCallback);
    fn dicta_stop(callback: NativeCallback);
}

pub(crate) fn start_recording(output_path: &str, callback: NativeCallback) -> Result<(), String> {
    let output_path = CString::new(output_path)
        .map_err(|_| "The recording path contains an unsupported character".to_string())?;
    unsafe { dicta_start(output_path.as_ptr(), callback) };
    Ok(())
}

pub(crate) fn stop_recording(callback: NativeCallback) -> Result<(), String> {
    unsafe { dicta_stop(callback) };
    Ok(())
}
