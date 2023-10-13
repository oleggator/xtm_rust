use libc;
use std::io;
use std::mem;
use std::os::unix::io::{AsRawFd, RawFd};
use tarantool::coio;
use tarantool::ffi::tarantool::CoIOFlags;

pub struct Notify(RawFd);

impl Notify {
    pub fn new(init: u32, is_semaphore: bool) -> io::Result<Self> {
        let flags = libc::EFD_NONBLOCK | libc::EFD_CLOEXEC;
        let flags = if is_semaphore {
            flags | libc::EFD_SEMAPHORE
        } else {
            flags
        };
        let rv = unsafe { libc::eventfd(init, flags) };
        if rv < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(rv))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        let rv = unsafe { libc::dup(self.0) };
        if rv < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(rv))
    }

    pub fn notified(&self) -> io::Result<u64> {
        let mut val: u64 = 0;
        let val_ptr: *mut u64 = &mut val;

        let rv = unsafe {
            libc::read(
                self.0,
                val_ptr as *mut std::ffi::c_void,
                mem::size_of::<u64>(),
            )
        };
        if rv < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(val)
    }

    pub fn notify(&self, val: u64) -> io::Result<()> {
        let val_ptr: *const u64 = &val;

        let rv = unsafe {
            libc::write(
                self.0,
                val_ptr as *const std::ffi::c_void,
                mem::size_of::<u64>(),
            )
        };
        if rv < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn notified_coio(&self, timeout: f64) -> io::Result<u64> {
        coio::coio_wait(self.as_raw_fd(), CoIOFlags::READ, timeout)?;
        self.notified()
    }

    pub fn notify_coio(&self, val: u64, timeout: f64) -> io::Result<()> {
        coio::coio_wait(self.as_raw_fd(), CoIOFlags::WRITE, timeout)?;
        self.notify(val)
    }
}

impl Drop for Notify {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

impl AsRawFd for Notify {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}
