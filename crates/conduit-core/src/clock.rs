//! Injectable time source for tests and production.

use std::time::Instant;

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_returns_instant() {
        let clock = SystemClock;
        let t0 = clock.now();
        let t1 = clock.now();
        assert!(t1 >= t0);
    }
}
