use std::ffi::c_void;
use std::io;
use std::os::raw::{c_int, c_uint};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const c_void, len: c_uint) -> c_int;
    fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, len: c_uint)
        -> c_int;
}

#[cfg(target_arch = "mips")]
const NONBLOCK: c_int = 0x80;
#[cfg(not(target_arch = "mips"))]
const NONBLOCK: c_int = 0o4000;
const CLOEXEC: c_int = 0o2000000;

#[cfg(target_arch = "mips")]
pub(crate) const STREAM: c_int = 2;
#[cfg(not(target_arch = "mips"))]
pub(crate) const STREAM: c_int = 1;
pub(crate) const RAW: c_int = 3;

pub(crate) fn open(domain: c_int, kind: c_int, protocol: c_int) -> io::Result<OwnedFd> {
    if !cfg!(all(
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "mips"
        )
    )) {
        return Err(io::ErrorKind::Unsupported.into());
    }
    let fd = unsafe { socket(domain, kind | NONBLOCK | CLOEXEC, protocol) };
    status(fd)?;
    // Linux socket returned a newly owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

// Safety: address must have the ABI layout of the socket's address family.
pub(crate) unsafe fn bind_to<T>(fd: &OwnedFd, address: &T) -> io::Result<()> {
    status(unsafe {
        bind(
            fd.as_raw_fd(),
            (address as *const T).cast(),
            std::mem::size_of::<T>() as c_uint,
        )
    })
}

// Safety: value must have the layout expected by the socket option.
pub(crate) unsafe fn option<T>(
    fd: &OwnedFd,
    level: c_int,
    name: c_int,
    value: &T,
) -> io::Result<()> {
    status(unsafe {
        setsockopt(
            fd.as_raw_fd(),
            level,
            name,
            (value as *const T).cast(),
            std::mem::size_of::<T>() as c_uint,
        )
    })
}

fn status(result: c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
