use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

pub(crate) fn open_attribute(path: &Path) -> io::Result<File> {
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a regular sysfs attribute",
        ));
    }
    let file = OpenOptions::new().read(true).open(path)?;
    let after = file.metadata()?;
    use std::os::unix::fs::MetadataExt;
    if !after.file_type().is_file() || (before.dev(), before.ino()) != (after.dev(), after.ino()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sysfs attribute changed during open",
        ));
    }
    Ok(file)
}

pub(crate) fn name_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}
