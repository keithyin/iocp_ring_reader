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

use crate::buffer::{Buffer, ReaderBufferStatus};
use crate::utils::{get_file_size, str_to_wide};

#[derive(Debug, Default, Clone, Copy)]
struct DataPos {
    buf_idx: usize,
    offset: usize,
}

pub struct SequentialReader {
    handle: *mut c_void,
    iocp: *mut c_void,
    buffers: Vec<crate::buffer::Buffer>,
    buffers_status: Vec<ReaderBufferStatus>,
    buffer_size: usize,
    data_pos: DataPos,
    file_pos_cursor: u64,
    file_size: u64,
    init_flag: bool,
    pendding: usize,
}

impl SequentialReader {
    pub fn new(fpath: &str, start_pos: u64, buffer_size: usize, num_buffer: usize) -> Self {
        assert!(buffer_size % 4096 == 0);

        let file_size = get_file_size(fpath);
        let fpath = str_to_wide(fpath);

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
            panic!("invalid handle value");
        }

        // TODO: clean up resources
        let iocp =
            unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, std::ptr::null_mut(), 0, 0) };

        if iocp == std::ptr::null_mut() {
            panic!("invalid iocp 1");
        }

        let iocp = unsafe { CreateIoCompletionPort(handle, iocp, 0, 0) };

        if iocp == std::ptr::null_mut() {
            panic!("invalid iocp 1");
        }

        let data_pose = DataPos {
            buf_idx: 0,
            offset: (start_pos % 4096) as usize,
        };
        let file_pos_cursor = start_pos - data_pose.offset as u64;

        let buffers = (0..num_buffer)
            .into_iter()
            .map(|idx| Buffer::new(buffer_size, idx))
            .collect();
        Self {
            handle: handle,
            iocp: iocp,
            buffers: buffers,
            buffers_status: vec![ReaderBufferStatus::default(); num_buffer],
            buffer_size: buffer_size,
            data_pos: data_pose,
            file_pos_cursor: file_pos_cursor,
            file_size: file_size,
            init_flag: false,
            pendding: 0,
        }
    }

    pub fn read2buf(&mut self, buf: &mut [u8]) {
        let req_len = buf.len();
        self.wait_inner_buf_ready();
        let mut remaining_bytes = req_len;

        let mut fill_pos = 0;
        while remaining_bytes > 0 {
            let cur_buf_read_n = remaining_bytes.min(self.buffer_size - self.data_pos.offset);
            unsafe {
                std::ptr::copy(
                    self.buffers[self.data_pos.buf_idx]
                        .data
                        .add(self.data_pos.offset),
                    buf.as_mut_ptr().add(fill_pos),
                    cur_buf_read_n,
                );
            }

            self.data_pos.offset += cur_buf_read_n;
            fill_pos += cur_buf_read_n;
            remaining_bytes -= cur_buf_read_n;

            if self.data_pos.offset == self.buffer_size {
                self.submit_read_event(self.data_pos.buf_idx);

                self.data_pos.buf_idx += 1;
                self.data_pos.buf_idx %= self.buffers.len();
                self.data_pos.offset = 0;
            }

            if remaining_bytes > 0 {
                self.wait_inner_buf_ready();
            }
        }
    }

    fn wait_inner_buf_ready(&mut self) -> Option<()> {
        if self.buffers_status[self.data_pos.buf_idx] == ReaderBufferStatus::Ready4Read {
            return Some(());
        }

        if !self.init_flag {
            for buf_idx in 0..self.buffers.len() {
                self.submit_read_event(buf_idx);
            }
            self.init_flag = true;
        }

        while self.pendding > 0 {
            let mut bytes_transferred: u32 = 0;
            let mut completion_key: usize = 0;
            let mut pov: *mut OVERLAPPED = std::ptr::null_mut();
            let ok = unsafe {
                GetQueuedCompletionStatus(
                    self.iocp,
                    &mut bytes_transferred as *mut u32,
                    &mut completion_key as *mut usize,
                    &mut pov as *mut *mut OVERLAPPED,
                    INFINITE,
                )
            };

            if pov == std::ptr::null_mut() {
                panic!("pov is null");
            }

            let task: *mut Buffer = pov as *mut Buffer;
            let idx = unsafe { (*task).idx };
            self.buffers_status[idx] = ReaderBufferStatus::Ready4Read;
            self.pendding -= 1;
            if self.buffers_status[self.data_pos.buf_idx] == ReaderBufferStatus::Ready4Read {
                break;
            }
        }

        Some(())
    }

    fn submit_read_event(&mut self, buf_idx: usize) {
        if self.file_pos_cursor >= self.file_size {
            self.buffers_status[buf_idx] = ReaderBufferStatus::Invalid;
            return;
        }

        if (self.file_pos_cursor + self.buffer_size as u64) >= self.file_size {
            // use other method to read the remaining data
            return;
        }

        let lo = (self.file_pos_cursor & 0xFFFF_FFFF) as u32;
        let hi = (self.file_pos_cursor >> 32) as u32;
        // self.buffers[buf_idx].overlapped.Pointer = std::ptr::null_mut(); // not used
        self.buffers[buf_idx].overlapped.Anonymous.Pointer = std::ptr::null_mut();

        self.buffers[buf_idx].overlapped.Anonymous.Anonymous.Offset = lo;

        self.buffers[buf_idx]
            .overlapped
            .Anonymous
            .Anonymous
            .OffsetHigh = hi;

        self.buffers[buf_idx].overlapped.Internal = 0;
        self.buffers[buf_idx].overlapped.InternalHigh = 0;

        self.buffers[buf_idx].offset = self.file_pos_cursor;
        self.buffers[buf_idx].len = 0;

        let ok = unsafe {
            ReadFile(
                self.handle,
                self.buffers[buf_idx].data as *mut _,
                self.buffer_size as u32,
                std::ptr::null_mut(), // lpNumberOfBytesRead = NULL for async
                &mut self.buffers[buf_idx].overlapped as *mut _,
            )
        };

        // if ok == 0 {
        //     panic!("ReadFile error");
        // }

        self.pendding += 1;

        self.file_pos_cursor += self.buffer_size as u64;
    }
}

impl Drop for SequentialReader {
    fn drop(&mut self) {
        unsafe {
            if self.handle != INVALID_HANDLE_VALUE {
                CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
mod test {}
