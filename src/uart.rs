use crate::common::Deadline;
use std::path::Path;

pub(crate) const DEFAULT_BYTES: u64 = 4096;
pub(crate) const MAX_BYTES: u64 = 65_536;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;

#[cfg(all(
    target_os = "linux",
    any(target_env = "gnu", target_env = "musl"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "mips",
    )
))]
mod linux;

#[cfg(all(
    target_os = "linux",
    any(target_env = "gnu", target_env = "musl"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "mips",
    )
))]
pub(crate) fn run(device: &Path, bytes: usize, deadline: Deadline) -> Result<(), String> {
    linux::run(device, bytes, deadline)
}

#[cfg(not(all(
    target_os = "linux",
    any(target_env = "gnu", target_env = "musl"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "mips",
    )
)))]
pub(crate) fn run(_device: &Path, _bytes: usize, _deadline: Deadline) -> Result<(), String> {
    Err("UART is unavailable for this Linux target's tty ABI".into())
}
