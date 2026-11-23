pub(crate) const DEFAULT_FRAMES: u64 = 120;
pub(crate) const MAX_FRAMES: u64 = 10_000;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    any(target_env = "gnu", target_env = "musl")
))]
mod linux;

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    any(target_env = "gnu", target_env = "musl")
))]
pub(crate) use linux::run;

#[cfg(not(all(
    target_os = "linux",
    target_pointer_width = "64",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    any(target_env = "gnu", target_env = "musl")
)))]
pub(crate) fn run(
    _path: &std::path::Path,
    _frames: u64,
    _deadline: crate::common::Deadline,
    _hdmi: bool,
) -> Result<(), String> {
    Err("V4L2 capture requires a supported 64-bit Linux V4L2 buffer ABI (x86-64 or AArch64)".into())
}
