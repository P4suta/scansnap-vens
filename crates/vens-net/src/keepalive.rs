use std::net::SocketAddrV4;

use vens_core::{
    discovery::{KeepaliveReply, Probe},
    keepalive::{Due, Elapsed, Schedule},
};

use crate::search::Datagram;

const DATAGRAMS_PER_TICK: u8 = 16;
const DATAGRAM_CAPACITY: usize = 512;
const PROBE_LENGTH: usize = 32;

/// Keeps a held reserve alive between the steps of a job.
pub trait Keepalive {
    fn tick(&mut self, now: Elapsed);
}

/// Sends nothing, for a caller that holds the reserve too briefly to need a keepalive.
#[derive(Debug, Default, Clone, Copy)]
pub struct Idle;

impl Keepalive for Idle {
    fn tick(&mut self, _: Elapsed) {}
}

/// Sends keepalive probes over a datagram port on the core schedule and counts the scanner's answers.
///
/// The port must not block, because a tick runs between the commands of a job.
/// A port failure stays with the driver for the caller, and it never interrupts the job.
pub struct DatagramKeepalive<D: Datagram> {
    port: D,
    frame: [u8; PROBE_LENGTH],
    target: SocketAddrV4,
    schedule: Schedule,
    replies: u32,
    failure: Option<D::Error>,
}

impl<D: Datagram> DatagramKeepalive<D> {
    #[must_use]
    pub fn new(port: D, probe: Probe, target: SocketAddrV4, now: Elapsed) -> Self {
        Self {
            port,
            frame: probe.encode(),
            target,
            schedule: Schedule::starting(now),
            replies: 0,
            failure: None,
        }
    }

    /// Counts the keepalive answers that the scanner has sent so far.
    #[must_use]
    pub const fn replies(&self) -> u32 {
        self.replies
    }

    /// Returns the port and the first failure that a tick met.
    #[must_use]
    pub fn into_parts(self) -> (D, Option<D::Error>) {
        (self.port, self.failure)
    }

    fn keep(&mut self, error: D::Error) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }

    fn read_answers(&mut self) {
        let mut buffer = [0; DATAGRAM_CAPACITY];
        for _ in 0..DATAGRAMS_PER_TICK {
            match self.port.receive(&mut buffer) {
                Ok(Some(length)) => {
                    let answer = buffer
                        .get(..length)
                        .is_some_and(|datagram| KeepaliveReply::decode(datagram).is_ok());
                    if answer {
                        self.schedule = self.schedule.answered();
                        self.replies = self.replies.saturating_add(1);
                    }
                }
                Ok(None) => return,
                Err(error) => {
                    self.keep(error);
                    return;
                }
            }
        }
    }
}

impl<D: Datagram> Keepalive for DatagramKeepalive<D> {
    fn tick(&mut self, now: Elapsed) {
        self.read_answers();
        let (schedule, due) = self.schedule.poll(now);
        self.schedule = schedule;
        match due {
            Due::Send => {
                if let Err(error) = self.port.send(&self.frame, self.target) {
                    self.keep(error);
                }
            }
            Due::Wait => {}
        }
    }
}
