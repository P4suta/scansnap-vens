use std::{net::SocketAddrV4, sync::mpsc};

use crate::{
    channel::Stream,
    search::Datagram,
    trace::{Channel, Direction, Event},
};

/// Passes every transfer through to a port and logs each one that succeeds with at least one byte.
///
/// Logging stops when the listener is gone, and it never fails a transfer.
#[derive(Debug)]
pub struct Recorder<P> {
    port: P,
    channel: Channel,
    events: mpsc::Sender<Event>,
}

impl<P> Recorder<P> {
    #[must_use]
    pub const fn new(port: P, channel: Channel, events: mpsc::Sender<Event>) -> Self {
        Self {
            port,
            channel,
            events,
        }
    }

    #[must_use]
    pub fn into_port(self) -> P {
        self.port
    }

    fn log(&self, direction: Direction, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let _ = self.events.send(Event {
            channel: self.channel,
            direction,
            bytes: bytes.to_vec(),
        });
    }
}

impl<S: Stream> Stream for Recorder<S> {
    type Error = S::Error;

    fn send(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.port.send(bytes)?;
        self.log(Direction::Sent, bytes);
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), S::Error> {
        self.port.receive(buffer)?;
        self.log(Direction::Received, buffer);
        Ok(())
    }
}

impl<D: Datagram> Datagram for Recorder<D> {
    type Error = D::Error;

    fn send(&mut self, frame: &[u8], target: SocketAddrV4) -> Result<(), D::Error> {
        self.port.send(frame, target)?;
        self.log(Direction::Sent, frame);
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, D::Error> {
        let received = self.port.receive(buffer)?;
        if let Some(datagram) = received.and_then(|length| buffer.get(..length)) {
            self.log(Direction::Received, datagram);
        }
        Ok(received)
    }
}
