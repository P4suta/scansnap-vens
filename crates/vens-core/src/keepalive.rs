const INTERVAL: u64 = 15_000;
const RETRY: u64 = 500;
const ATTEMPTS: u8 = 3;

/// Milliseconds since an arbitrary start, taken from a monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Elapsed(u64);

impl Elapsed {
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    #[must_use]
    pub const fn millis(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    Send,
    Wait,
}

/// Decides when a host that holds the reserve sends keepalive packets.
///
/// A round starts every 15 s from the start, and each round sends up to 3 packets half a second apart until the scanner answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    start: u64,
    round: u64,
    sent: u8,
    answered: bool,
    last: u64,
}

impl Schedule {
    #[must_use]
    pub const fn starting(now: Elapsed) -> Self {
        Self {
            start: now.0,
            round: now.0,
            sent: 0,
            answered: false,
            last: now.0,
        }
    }

    /// Records the scanner's answer, which ends the current round.
    #[must_use]
    pub const fn answered(self) -> Self {
        Self {
            answered: true,
            ..self
        }
    }

    /// Tells whether a keepalive goes out at `now`, and returns the schedule that counts it.
    #[must_use]
    pub fn poll(self, now: Elapsed) -> (Self, Due) {
        let round = self.round_at(now.0);
        let schedule = if round > self.round {
            Self {
                round,
                sent: 0,
                answered: false,
                last: round,
                ..self
            }
        } else {
            self
        };
        let spaced = schedule.sent == 0 || now.0 >= schedule.last.saturating_add(RETRY);
        if schedule.answered || schedule.sent >= ATTEMPTS || !spaced {
            return (schedule, Due::Wait);
        }
        (
            Self {
                sent: schedule.sent.saturating_add(1),
                last: now.0,
                ..schedule
            },
            Due::Send,
        )
    }

    fn round_at(self, now: u64) -> u64 {
        let rounds = now
            .saturating_sub(self.start)
            .checked_div(INTERVAL)
            .unwrap_or(0);
        self.start.saturating_add(rounds.saturating_mul(INTERVAL))
    }
}
