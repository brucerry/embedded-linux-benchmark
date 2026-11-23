use crate::common::Deadline;
use std::io;
use std::os::raw::{c_int, c_short, c_ulong};
use std::os::unix::io::RawFd;

pub(crate) const READ: c_short = 0x01;
pub(crate) const WRITE: c_short = 0x04;

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

extern "C" {
    fn poll(fds: *mut PollFd, count: c_ulong, timeout_ms: c_int) -> c_int;
}

pub(crate) fn wait(fd: RawFd, events: c_short, deadline: Deadline) -> Result<(), String> {
    let mut entry = PollFd {
        fd,
        events,
        revents: 0,
    };
    loop {
        let millis = (deadline.remaining()?.as_nanos() + 999_999) / 1_000_000;
        let millis = millis.min(c_int::MAX as u128) as c_int;
        // The buffer contains one pollfd, and the timeout is bounded by the deadline.
        let result = unsafe { poll(&mut entry, 1, millis) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(format!("poll failed: {error}"));
            }
        }
    }
}
