use crate::common::{Deadline, RunError, MIB};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const MAX_MIB: u64 = 64;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 10_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 60_000;
const CHUNK_BYTES: usize = 64 * 1_024;
static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn directory(path: &Path) -> Result<PathBuf, String> {
    let directory = path.canonicalize().map_err(|error| {
        format!(
            "cannot open storage directory '{}': {error}",
            path.display()
        )
    })?;
    if !directory.is_dir() {
        return Err(format!(
            "storage path '{}' must be a directory",
            path.display()
        ));
    }
    Ok(directory)
}

struct ScratchFile {
    path: PathBuf,
    file: File,
    removed: bool,
}

impl ScratchFile {
    fn create(directory: &Path) -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        for _ in 0..16 {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(
                ".embedded-linux-benchmark-{pid}-{stamp}-{sequence}"
            ));
            match OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file,
                        removed: false,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no unused scratch filename",
        ))
    }

    fn remove(&mut self) -> Result<(), String> {
        fs::remove_file(&self.path).map_err(|error| {
            format!(
                "cannot remove scratch file '{}': {error}",
                self.path.display()
            )
        })?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        if !self.removed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn measure(
    file: &mut File,
    bytes: u64,
    sync: bool,
    deadline: Deadline,
    written: &mut u64,
) -> Result<(Duration, Duration), RunError> {
    let payload = [0xa5u8; CHUNK_BYTES];
    let mut buffer = [0u8; CHUNK_BYTES];
    let start = Instant::now();
    let mut remaining = bytes;
    while remaining > 0 {
        deadline.check()?;
        let len = remaining.min(CHUNK_BYTES as u64) as usize;
        match file.write(&payload[..len]) {
            Ok(0) => {
                return Err(RunError::Failed(
                    "storage write failed: wrote zero bytes".into(),
                ))
            }
            Ok(count) => {
                remaining -= count as u64;
                *written += count as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("storage write failed: {error}").into()),
        }
    }
    if sync {
        deadline.check()?;
        file.sync_data()
            .map_err(|error| format!("storage sync_data failed: {error}"))?;
    }
    let write_elapsed = start.elapsed();
    deadline.check()?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("storage seek failed: {error}"))?;

    let start = Instant::now();
    let mut remaining = bytes;
    while remaining > 0 {
        deadline.check()?;
        let len = remaining.min(CHUNK_BYTES as u64) as usize;
        file.read_exact(&mut buffer[..len])
            .map_err(|error| format!("storage read failed: {error}"))?;
        if buffer[..len] != payload[..len] {
            return Err(RunError::Failed("storage read verification failed".into()));
        }
        remaining -= len as u64;
    }
    let read_elapsed = start.elapsed();
    deadline.check()?;
    Ok((write_elapsed, read_elapsed))
}

pub(crate) fn benchmark(
    directory: &Path,
    bytes: u64,
    sync: bool,
    deadline: Deadline,
    written: &mut u64,
) -> Result<(Duration, Duration), RunError> {
    deadline.check()?;
    let mut scratch = ScratchFile::create(directory).map_err(|error| {
        format!(
            "cannot create scratch file in '{}': {error}",
            directory.display()
        )
    })?;
    let result = measure(&mut scratch.file, bytes, sync, deadline, written);
    let cleanup = scratch.remove();
    let timings = match (result, cleanup) {
        (Err(error), Err(cleanup)) => return Err(format!("{error}. {cleanup}").into()),
        (Err(error), Ok(())) => return Err(error),
        (Ok(_), Err(error)) => return Err(error.into()),
        (Ok(timings), Ok(())) => timings,
    };
    deadline.check()?;
    Ok(timings)
}

pub(crate) fn run(
    directory: &Path,
    bytes: u64,
    sync: bool,
    deadline: Deadline,
) -> Result<(), String> {
    let (write_elapsed, read_elapsed) = benchmark(directory, bytes, sync, deadline, &mut 0)?;
    describe(directory, bytes, sync, write_elapsed, read_elapsed, true);
    Ok(())
}

pub(crate) fn describe(
    directory: &Path,
    bytes: u64,
    sync: bool,
    write_elapsed: Duration,
    read_elapsed: Duration,
    record: bool,
) {
    let write_mode = if sync {
        "buffered, includes sync_data"
    } else {
        "buffered, page cache eligible"
    };
    let write_seconds = write_elapsed.as_secs_f64();
    let read_seconds = read_elapsed.as_secs_f64();
    let mib = bytes as f64 / MIB as f64;
    if record {
        crate::report::metric("Sequential write throughput", mib / write_seconds, "MiB/s");
        crate::report::metric("Sequential read throughput", mib / read_seconds, "MiB/s");
    }
    crate::report::emit(
        record,
        format_args!(
            "Storage throughput (sequential filesystem I/O)\n\
         Directory: {}\n\
         Write mode: {write_mode}\n\
         Write bytes: {bytes}\n\
         Write elapsed: {write_seconds:.6} s\n\
         Write throughput: {:.2} MiB/s\n\
         Read mode: buffered, page cache eligible, includes verification\n\
         Read bytes: {bytes}\n\
         Read elapsed: {read_seconds:.6} s\n\
         Read throughput: {:.2} MiB/s",
            directory.display(),
            mib / write_seconds,
            mib / read_seconds,
        ),
    );
}
