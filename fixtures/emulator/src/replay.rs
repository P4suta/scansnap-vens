use std::{collections::VecDeque, net::SocketAddrV4};

use vens_net::{
    channel::Stream,
    search::Datagram,
    trace::{Channel, Direction, Event},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    /// The host sent a byte that the recording doesn't have at `offset` of the bytes sent on the channel.
    Diverged {
        offset: usize,
        expected: u8,
        actual: u8,
    },
    /// The host sent while the recording holds a reply, or read while it holds a request.
    OutOfOrder {
        offset: usize,
    },
    /// The host went on after the recording ended.
    Exhausted,
    DatagramLength {
        expected: usize,
        actual: usize,
    },
}

#[derive(Debug)]
struct Segment {
    direction: Direction,
    bytes: VecDeque<u8>,
}

/// Plays one recorded channel back as a byte stream.
///
/// The host's bytes must match the recording in order, whatever the size of each call.
#[derive(Debug)]
pub struct Replay {
    segments: VecDeque<Segment>,
    sent: usize,
}

impl Replay {
    #[must_use]
    pub fn new(events: &[Event], channel: Channel) -> Self {
        let mut segments: VecDeque<Segment> = VecDeque::new();
        for event in events.iter().filter(|event| event.channel == channel) {
            match segments.back_mut() {
                Some(segment) if segment.direction == event.direction => {
                    segment.bytes.extend(&event.bytes);
                }
                Some(_) | None => segments.push_back(Segment {
                    direction: event.direction,
                    bytes: event.bytes.iter().copied().collect(),
                }),
            }
        }
        Self { segments, sent: 0 }
    }

    /// Tells whether the host has sent and read every recorded byte.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.segments.is_empty()
    }

    fn next_byte(&mut self, direction: Direction) -> Result<u8, ReplayError> {
        let offset = self.sent;
        let segment = self.segments.front_mut().ok_or(ReplayError::Exhausted)?;
        if segment.direction != direction {
            return Err(ReplayError::OutOfOrder { offset });
        }
        let byte = segment.bytes.pop_front().ok_or(ReplayError::Exhausted)?;
        if segment.bytes.is_empty() {
            self.segments.pop_front();
        }
        Ok(byte)
    }
}

impl Stream for Replay {
    type Error = ReplayError;

    fn send(&mut self, bytes: &[u8]) -> Result<(), ReplayError> {
        for &actual in bytes {
            let offset = self.sent;
            let expected = self.next_byte(Direction::Sent)?;
            if expected != actual {
                return Err(ReplayError::Diverged {
                    offset,
                    expected,
                    actual,
                });
            }
            self.sent = self.sent.saturating_add(1);
        }
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), ReplayError> {
        for slot in buffer {
            *slot = self.next_byte(Direction::Received)?;
        }
        Ok(())
    }
}

/// Plays the recorded discovery datagrams back one by one.
#[derive(Debug)]
pub struct DatagramReplay {
    events: VecDeque<Event>,
    sent: usize,
}

impl DatagramReplay {
    #[must_use]
    pub fn new(events: &[Event]) -> Self {
        Self {
            events: events
                .iter()
                .filter(|event| event.channel == Channel::Discovery)
                .cloned()
                .collect(),
            sent: 0,
        }
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.events.is_empty()
    }
}

impl Datagram for DatagramReplay {
    type Error = ReplayError;

    fn send(&mut self, frame: &[u8], _: SocketAddrV4) -> Result<(), ReplayError> {
        let expected = match self.events.front() {
            None => return Err(ReplayError::Exhausted),
            Some(event) if event.direction == Direction::Received => {
                return Err(ReplayError::OutOfOrder { offset: self.sent });
            }
            Some(event) => &event.bytes,
        };
        let difference = expected
            .iter()
            .zip(frame)
            .enumerate()
            .find(|(_, (expected, actual))| expected != actual);
        if let Some((index, (&expected, &actual))) = difference {
            return Err(ReplayError::Diverged {
                offset: self.sent.saturating_add(index),
                expected,
                actual,
            });
        }
        if expected.len() != frame.len() {
            return Err(ReplayError::DatagramLength {
                expected: expected.len(),
                actual: frame.len(),
            });
        }
        self.events.pop_front();
        self.sent = self.sent.saturating_add(frame.len());
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, ReplayError> {
        let Some(event) = self.events.front() else {
            return Ok(None);
        };
        if event.direction == Direction::Sent {
            return Ok(None);
        }
        let length = event.bytes.len().min(buffer.len());
        buffer
            .iter_mut()
            .zip(&event.bytes)
            .for_each(|(slot, byte)| *slot = *byte);
        self.events.pop_front();
        Ok(Some(length))
    }
}
