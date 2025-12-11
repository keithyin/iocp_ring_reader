use std::ffi::c_void;

use windows_sys::Win32::System::{
    IO::OVERLAPPED,
    Memory::{MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree},
};
/// This struct places OVERLAPPED as the first field so the pointer to OVERLAPPED
/// we get back from GetQueuedCompletionStatus can be cast back to *mut Buffer.
#[repr(C)]
pub struct Buffer {
    overlapped: OVERLAPPED, // must be first
    offset: u64,            // file offset for this buffer
    len: usize,             // bytes actually read
    data: *mut u8,          // pointer to buffer storage
}

impl Buffer {
    pub fn new(size: usize) -> Box<Self> {
        // allocate Vec<u8> for data and leak into raw pointer.
        // For FILE_FLAG_NO_BUFFERING you'd need aligned allocation and sizes multiple of sector.
        let p = unsafe {
            VirtualAlloc(
                std::ptr::null_mut(),
                size,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            ) as *mut u8
        };

        let ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        // For overlapped.hEvent we leave 0; we use IOCP to get completions.
        Box::new(Buffer {
            overlapped: ov,
            offset: 0,
            len: 0,
            data: p,
        })
    }

    fn free_data(&mut self) {
        // reconstruct Vec to free memory
        unsafe {
            VirtualFree(self.data as *mut c_void, 0, MEM_RELEASE);
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.free_data();
    }
}
