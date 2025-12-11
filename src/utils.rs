use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

/// convert Rust &str path to wide null-terminated Vec<u16>
pub fn str_to_wide(path: &str) -> Vec<u16> {
    let mut v: Vec<u16> = OsStr::new(path).encode_wide().collect();
    v.push(0);
    v
}
