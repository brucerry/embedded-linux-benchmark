use crate::common::{Deadline, RunError, Sample, MIB};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_MIB: u64 = 8;
pub(crate) const MAX_MIB: u64 = 64;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;
const CHUNK_BYTES: usize = 32 * 1_024;

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

fn pause(deadline: Deadline) -> Result<(), RunError> {
    thread::sleep(deadline.remaining()?.min(Duration::from_millis(1)));
    Ok(())
}

fn transferred(result: io::Result<usize>, operation: &str) -> Result<usize, RunError> {
    match result {
        Ok(0) => Err(format!("loopback {operation} ended before the byte budget").into()),
        Ok(count) => Ok(count),
        Err(error) if retryable(&error) => Ok(0),
        Err(error) => Err(format!("loopback {operation} failed: {error}").into()),
    }
}

fn connect(deadline: Deadline) -> Result<(TcpStream, TcpStream), RunError> {
    let connect = || -> io::Result<_> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = listener.local_addr()?;
        Ok((listener, endpoint))
    };
    let (listener, endpoint) =
        connect().map_err(|error| format!("cannot set up TCP loopback listener: {error}"))?;
    let sender = TcpStream::connect_timeout(&endpoint, deadline.remaining()?).map_err(|error| {
        if error.kind() == io::ErrorKind::TimedOut {
            RunError::Deadline
        } else {
            format!("cannot connect to TCP loopback listener: {error}").into()
        }
    })?;
    let peer = sender
        .local_addr()
        .map_err(|error| format!("cannot identify loopback sender: {error}"))?;
    let receiver = loop {
        deadline.check()?;
        match listener.accept() {
            Ok((stream, address)) if address == peer => break stream,
            Ok(_) => {
                return Err(RunError::Failed(
                    "unexpected peer on TCP loopback listener".into(),
                ))
            }
            Err(error) if retryable(&error) => pause(deadline)?,
            Err(error) => {
                return Err(format!("cannot accept TCP loopback connection: {error}").into())
            }
        }
    };
    sender
        .set_nonblocking(true)
        .and_then(|()| receiver.set_nonblocking(true))
        .map_err(|error| format!("cannot configure TCP loopback sockets: {error}"))?;
    Ok((sender, receiver))
}

pub(crate) fn benchmark(bytes: u64, deadline: Deadline) -> Result<Sample, RunError> {
    pump(bytes, Duration::ZERO, deadline, false).map(|(sample, _)| sample)
}

/// Keep the same loopback connection busy for a whole phase rather than
/// repeatedly reconnecting and filling the TCP TIME_WAIT table during a soak.
pub(crate) fn sustained(
    bytes: u64,
    minimum: Duration,
    deadline: Deadline,
) -> Result<(Sample, bool), RunError> {
    pump(bytes, minimum, deadline, true)
}

fn pump(
    bytes: u64,
    minimum: Duration,
    deadline: Deadline,
    partial: bool,
) -> Result<(Sample, bool), RunError> {
    let idle_limit = Duration::from_millis(DEFAULT_TIMEOUT_MS);
    let connection_deadline = if partial {
        deadline.capped(idle_limit)
    } else {
        deadline
    };
    let (mut sender, mut receiver) = connect(connection_deadline)?;
    let payload = [0xa5u8; CHUNK_BYTES];
    let mut buffer = [0u8; CHUNK_BYTES];
    let mut sent = 0u64;
    let mut received = 0u64;
    let start = Instant::now();
    let mut last_progress = start;
    // In a stress phase, bytes is the initial minimum traffic and the maximum
    // outstanding window. The memory used by the pump is fixed either way.
    while received < bytes || start.elapsed() < minimum {
        if deadline.check().is_err() {
            if partial {
                return Ok((
                    Sample {
                        count: received,
                        elapsed: start.elapsed(),
                    },
                    false,
                ));
            }
            return Err(RunError::Deadline);
        }
        let previous = (sent, received);
        let capacity = if partial {
            bytes - (sent - received)
        } else {
            bytes - sent
        };
        if capacity > 0 {
            let len = capacity.min(CHUNK_BYTES as u64) as usize;
            sent += transferred(sender.write(&payload[..len]), "send")? as u64;
        }
        if received < sent {
            let len = (sent - received).min(CHUNK_BYTES as u64) as usize;
            let count = transferred(receiver.read(&mut buffer[..len]), "receive")?;
            if buffer[..count] != payload[..count] {
                return Err(RunError::Failed(
                    "TCP loopback payload verification failed".into(),
                ));
            }
            received += count as u64;
        }
        if (sent, received) == previous {
            if partial && last_progress.elapsed() >= idle_limit {
                return Err(RunError::Failed(
                    "TCP loopback made no progress for five seconds".into(),
                ));
            }
            if let Err(error) = pause(deadline) {
                if partial {
                    return Ok((
                        Sample {
                            count: received,
                            elapsed: start.elapsed(),
                        },
                        false,
                    ));
                }
                return Err(error);
            }
        } else if partial {
            last_progress = Instant::now();
        }
    }
    let elapsed = start.elapsed();
    let full = deadline.check().is_ok();
    if !partial && !full {
        return Err(RunError::Deadline);
    }
    Ok((
        Sample {
            count: received,
            elapsed,
        },
        full,
    ))
}

pub(crate) fn run(bytes: u64, deadline: Deadline) -> Result<(), String> {
    let sample = benchmark(bytes, deadline)?;
    describe(&sample, true);
    Ok(())
}

pub(crate) fn describe(sample: &Sample, record: bool) {
    let received = sample.count;
    let seconds = sample.elapsed.as_secs_f64();
    if record {
        crate::report::metric(
            "TCP loopback throughput",
            received as f64 / (MIB as f64 * seconds),
            "MiB/s",
        );
    }
    crate::report::emit(
        record,
        format_args!(
            "Network throughput (TCP loopback, single thread)\n\
         Address: 127.0.0.1\n\
         Bytes received: {received}\n\
         Elapsed: {seconds:.6} s\n\
         Throughput: {:.2} MiB/s",
            received as f64 / (MIB as f64 * seconds),
        ),
    );
}
