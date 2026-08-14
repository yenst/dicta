use std::{ffi::CString, os::raw::c_char};

unsafe extern "C" {
    fn dicta_extract_audio(input_path: *const c_char, output_path: *const c_char) -> bool;
    fn dicta_extract_poster(input_path: *const c_char, output_path: *const c_char) -> bool;
}

pub(crate) fn extract_audio(input_path: &str, output_path: &str) -> bool {
    let Ok(input_path) = CString::new(input_path) else {
        return false;
    };
    let Ok(output_path) = CString::new(output_path) else {
        return false;
    };
    unsafe { dicta_extract_audio(input_path.as_ptr(), output_path.as_ptr()) }
}

pub(crate) fn extract_poster(input_path: &str, output_path: &str) -> bool {
    let Ok(input_path) = CString::new(input_path) else {
        return false;
    };
    let Ok(output_path) = CString::new(output_path) else {
        return false;
    };
    unsafe { dicta_extract_poster(input_path.as_ptr(), output_path.as_ptr()) }
}
