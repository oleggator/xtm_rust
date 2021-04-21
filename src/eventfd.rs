use std::{convert::TryFrom, io};
use std::os::unix::io::{AsRawFd, RawFd};
use std::mem;
use libc;
use tarantool::ffi::tarantool::CoIOFlags;
use tarantool::coio;
use tokio::io::unix::AsyncFd;

pub struct EventFd(RawFd);

impl EventFd {
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

    pub fn read(&self) -> std::io::Result<u64> {
        let mut val: u64 = 0;
        let val_ptr: *mut u64 = &mut val;

        let rv = unsafe {
            libc::read(self.0, val_ptr as *mut std::ffi::c_void, mem::size_of::<u64>())
        };
        if rv < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(val)
    }

    pub fn write(&self, val: u64) -> io::Result<()> {
        let val_ptr: *const u64 = &val;

        let rv = unsafe {
            libc::write(self.0, val_ptr as *const std::ffi::c_void, mem::size_of::<u64>())
        };
        if rv < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }

    pub fn coio_read(&self, timeout: f64) -> io::Result<u64> {
        coio::coio_wait(self.as_raw_fd(), CoIOFlags::READ, timeout)?;
        self.read()
    }

    pub fn coio_write(&self, val: u64, timeout: f64) -> io::Result<()> {
        coio::coio_wait(self.as_raw_fd(), CoIOFlags::WRITE, timeout)?;
        self.write(val)
    }
}

impl Drop for EventFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

impl AsRawFd for EventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

pub struct AsyncEventFd(AsyncFd<EventFd>);

impl AsyncEventFd {
    pub fn try_clone(&self) -> io::Result<Self> {
        let inner = self.0.get_ref().try_clone()?;
        Ok(AsyncEventFd(AsyncFd::new(inner)?))
    }

    pub async fn write(&self, val: u64) -> io::Result<()> {
        loop {
            let mut guard = self.0.writable().await?;

            match guard.try_io(|inner| inner.get_ref().write(val)) {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }
}

impl TryFrom<EventFd> for AsyncEventFd {
    type Error = io::Error;
    fn try_from(event_fd: EventFd) -> Result<Self, Self::Error> {
        Ok(Self(AsyncFd::new(event_fd)?))
    }
}

impl AsRawFd for AsyncEventFd {
    fn as_raw_fd(&self) -> RawFd {
        self.0.get_ref().0
    }
}
