#![expect(unsafe_code, reason = "the C ABI is raw pointers and exported symbols")]
#![allow(non_camel_case_types)]

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_char;
use std::panic::catch_unwind;
use std::ptr;

use shapeof_core::Target;

pub const SHAPEOF_OK: u32 = 0;
pub const SHAPEOF_INVALID_UTF8: u32 = 1;
pub const SHAPEOF_INVALID_ARGUMENT: u32 = 2;
pub const SHAPEOF_INTERNAL: u32 = 3;

pub const SHAPEOF_OPENAI: u32 = 0;
pub const SHAPEOF_ANTHROPIC: u32 = 1;
pub const SHAPEOF_PLAIN: u32 = 2;

#[repr(C)]
#[derive(Debug)]
pub struct shapeof_report {
    pub status: u32,
    pub data: *mut u8,
    pub len: usize,
}

/// # Safety
/// `p` is null with `len` zero, or points to `len` readable bytes.
unsafe fn text<'a>(p: *const u8, len: usize) -> Option<&'a str> {
    let bytes = match (p.is_null(), len) {
        (_, 0) => &[][..],
        (true, _) => return None,
        // SAFETY: the caller passes `len` readable bytes at `p`.
        (false, _) => unsafe { std::slice::from_raw_parts(p, len) },
    };

    std::str::from_utf8(bytes).ok()
}

/// # Safety
/// `input` is null with `input_len` zero, or points to `input_len` readable
/// bytes; `out` points to a writable `shapeof_report`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shapeof_infer(input: *const u8, input_len: usize, target: u32, out: *mut shapeof_report) {
    let mut result = shapeof_report { status: SHAPEOF_OK, data: ptr::null_mut(), len: 0 };
    let target = match target {
        SHAPEOF_OPENAI => Some(Target::OpenAi),
        SHAPEOF_ANTHROPIC => Some(Target::Anthropic),
        SHAPEOF_PLAIN => Some(Target::Plain),
        _ => None,
    };

    // SAFETY: forwarded from the caller.
    match (unsafe { text(input, input_len) }, target) {
        (None, _) => result.status = SHAPEOF_INVALID_UTF8,
        (_, None) => result.status = SHAPEOF_INVALID_ARGUMENT,
        (Some(text), Some(target)) => match catch_unwind(|| shapeof_core::report(text, target)) {
            Ok(report) => {
                let report = report.into_bytes().into_boxed_slice();

                result.len = report.len();
                result.data = Box::into_raw(report).cast::<u8>();
            }
            Err(_) => result.status = SHAPEOF_INTERNAL,
        },
    }

    if !out.is_null() {
        // SAFETY: the caller passes a writable `shapeof_report`.
        unsafe { out.write(result) };
    }
}

/// # Safety
/// `report` is null or points to a `shapeof_report` that `shapeof_infer` wrote
/// and that has not been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shapeof_report_free(report: *mut shapeof_report) {
    // SAFETY: the caller passes null or a valid `shapeof_report`.
    let Some(report) = (unsafe { report.as_mut() }) else { return };

    if !report.data.is_null() {
        let bytes = ptr::slice_from_raw_parts_mut(report.data, report.len);
        // SAFETY: `data` came from `Box::into_raw` of exactly this many bytes.
        drop(unsafe { Box::from_raw(bytes) });
    }

    report.data = ptr::null_mut();
    report.len = 0;
}

#[unsafe(no_mangle)]
pub extern "C" fn shapeof_version() -> *const c_char {
    const VERSION: &std::ffi::CStr =
        match std::ffi::CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
            Ok(version) => version,
            Err(_) => panic!("the version has no NUL"),
        };

    VERSION.as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn shapeof_alloc(len: usize) -> *mut u8 {
    match Layout::array::<u8>(len) {
        // SAFETY: the layout is valid and not zero-sized.
        Ok(layout) if len > 0 => unsafe { alloc(layout) },
        _ => ptr::null_mut(),
    }
}

/// # Safety
/// `p` is null or came from `shapeof_alloc(len)` and has not been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn shapeof_dealloc(p: *mut u8, len: usize) {
    if let (false, Ok(layout)) = (p.is_null(), Layout::array::<u8>(len))
        && len > 0
    {
        // SAFETY: `p` came from `shapeof_alloc` with this layout.
        unsafe { dealloc(p, layout) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &[u8], target: u32) -> (u32, String) {
        let mut out = shapeof_report { status: 9, data: ptr::null_mut(), len: 0 };

        // SAFETY: the text and `out` are valid for the call.
        unsafe { shapeof_infer(text.as_ptr(), text.len(), target, &raw mut out) };

        let report = if out.data.is_null() {
            String::new()
        } else {
            // SAFETY: `shapeof_infer` wrote `len` bytes at `data`.
            String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(out.data, out.len) }).into_owned()
        };

        // SAFETY: `out` was written by `shapeof_infer`.
        unsafe { shapeof_report_free(&raw mut out) };
        (out.status, report)
    }

    #[test]
    fn reports_through_the_abi() {
        let (status, report) = run(br#"{"a": 1}"#, 0);

        assert_eq!(status, SHAPEOF_OK);
        assert!(report.starts_with("{\"schema\":\"{\\n  \\\"type\\\": \\\"object\\\""));
        assert_eq!(run(&[0xff], 0).0, SHAPEOF_INVALID_UTF8);
        assert_eq!(run(b"{}", 7).0, SHAPEOF_INVALID_ARGUMENT);
    }
}
