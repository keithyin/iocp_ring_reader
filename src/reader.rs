#![allow(non_snake_case)]
use std::boxed::Box;
use std::convert::TryInto;
use std::ffi::{OsStr, c_void};
use std::io::{self, Write};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::slice;
use windows_sys::Win32::Foundation::{BOOL, GENERIC_READ};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OVERLAPPED, FILE_FLAG_SEQUENTIAL_SCAN,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows_sys::Win32::System::IO::{
    CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED, PostQueuedCompletionStatus,
};
use windows_sys::Win32::System::Threading::INFINITE;

use crate::utils::str_to_wide;

#[derive(Debug, Default, Clone, Copy)]
struct DataPos {
    buf_idx: usize,
    offset: usize
}

pub struct Reader {
    handle: *mut c_void,
    iocp: *mut c_void,
    buffers: Vec<crate::buffer::Buffer>,
    data_pos: DataPos
}

impl Reader {
    pub fn new(fname: &str) -> Self {
        let fpath = str_to_wide(fname);
        let handle = unsafe {
            CreateFileW(
                fpath.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED | FILE_FLAG_SEQUENTIAL_SCAN,
                std::ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            panic!("invalid handle value")
        }

        let iocp = unsafe {
            CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 0)
        };

        let iocp = unsafe {
            CreateIoCompletionPort(handle, iocp, 0, 0)
        };


        Self { handle: handle, iocp: iocp , buffers: vec![], data_pos: DataPos::default()}
    }
}
