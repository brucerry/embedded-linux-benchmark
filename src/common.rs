use std::fmt;
use std::time::{Duration, Instant};

pub(crate) const MIB: u64 = 1_048_576;
pub(crate) const DEFAULT_DURATION_MS: u64 = 1_000;
pub(crate) const MAX_DURATION_MS: u64 = 60_000;

#[derive(Debug)]
pub(crate) enum RunError {
    Deadline,
    Failed(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Deadline => "time limit reached",
            Self::Failed(message) => message,
        })
    }
}

impl From<String> for RunError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<RunError> for String {
    fn from(error: RunError) -> Self {
        error.to_string()
    }
}

#[derive(Default)]
pub(crate) struct Sample {
    pub(crate) count: u64,
    pub(crate) elapsed: Duration,
}

impl Sample {
    pub(crate) fn add(&mut self, sample: Self) {
        self.count = self.count.saturating_add(sample.count);
        self.elapsed = self.elapsed.saturating_add(sample.elapsed);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn after(duration: Duration) -> Self {
        Self(Instant::now() + duration)
    }

    pub(crate) fn capped(self, duration: Duration) -> Self {
        Self(self.0.min(Instant::now() + duration))
    }

    pub(crate) fn remaining(self) -> Result<Duration, RunError> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or(RunError::Deadline)
    }

    pub(crate) fn check(self) -> Result<(), RunError> {
        self.remaining().map(|_| ())
    }
}

pub(crate) fn measure(
    minimum: Duration,
    deadline: Option<Deadline>,
    mut work: impl FnMut(),
) -> Result<Sample, RunError> {
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
            return Ok(Sample {
                count: rounds,
                elapsed,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_totals_saturate_at_the_counter_limits() {
        let mut totals = Sample {
            count: u64::MAX - 1,
            elapsed: Duration::MAX,
        };
        totals.add(Sample {
            count: 2,
            elapsed: Duration::from_secs(1),
        });
        assert_eq!(totals.count, u64::MAX);
        assert_eq!(totals.elapsed, Duration::MAX);
    }
}
