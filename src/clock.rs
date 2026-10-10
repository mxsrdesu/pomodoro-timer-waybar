use rustix::time::{ClockId, clock_gettime};

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub monotonic_ms: u64,
    pub boottime_ms: u64,
}

impl Sample {
    pub fn now() -> Self {
        // Bracket boottime to keep scheduling delays between clock reads from
        // looking like suspend. Retry a sample if the reads took too long.
        loop {
            let before = milliseconds(ClockId::Monotonic);
            let boot = milliseconds(ClockId::Boottime);
            let after = milliseconds(ClockId::Monotonic);
            if after.saturating_sub(before) < 10 {
                return Self {
                    monotonic_ms: before + (after - before) / 2,
                    boottime_ms: boot,
                };
            }
        }
    }

    pub fn sleep_offset_ms(self) -> u64 {
        self.boottime_ms.saturating_sub(self.monotonic_ms)
    }
}

fn milliseconds(clock: ClockId) -> u64 {
    let time = clock_gettime(clock);
    time.tv_sec as u64 * 1_000 + time.tv_nsec as u64 / 1_000_000
}
