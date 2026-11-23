use std::time::{Duration, Instant};

pub(crate) const MIB: u64 = 1_048_576;
pub(crate) const DEFAULT_DURATION_MS: u64 = 1_000;
pub(crate) const MAX_DURATION_MS: u64 = 60_000;

#[derive(Clone, Copy)]
pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn after(duration: Duration) -> Self {
        Self(Instant::now() + duration)
    }

    pub(crate) fn remaining(self) -> Result<Duration, String> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| "time limit reached".into())
    }

    pub(crate) fn check(self) -> Result<(), String> {
        self.remaining().map(|_| ())
    }
}

pub(crate) fn measure(
    minimum: Duration,
    deadline: Option<Deadline>,
    mut work: impl FnMut(),
) -> Result<(u64, Duration), String> {
    let start = Instant::now();
    let limit = match deadline {
        Some(deadline) => minimum.min(deadline.remaining()?),
        None => minimum,
    };
    let mut rounds = 0;
    loop {
        work();
        rounds += 1;
        let elapsed = start.elapsed();
        if elapsed >= limit {
            if elapsed < minimum {
                return Err("time limit reached".into());
            }
            return Ok((rounds, elapsed));
        }
    }
}
