use std::net::{Ipv4Addr, SocketAddrV4};

use vens_core::{Ipv4Address, Port, control::LocalTime, discovery::SearchReply};
use vens_net::{
    network::{Connection, Network},
    trace::{Channel, Direction, Event},
};

use crate::{
    redact::{HOST_ADDRESS, TRIGGER_PORT},
    replay::{DatagramReplay, Replay, ReplayError},
    session::Pauses,
};

/// Offers a recorded scanner through the network port.
///
/// The host facilities carry the documentation values that redaction writes into a fixture.
/// A connection reaches a replay only at the endpoint that the recorded search reply names for it.
#[derive(Debug)]
pub struct FixtureNetwork {
    events: Vec<Event>,
    command: SocketAddrV4,
    control: SocketAddrV4,
    time: LocalTime,
}

impl FixtureNetwork {
    /// Builds the network from a fixture, or returns `None` when the fixture holds no search reply.
    #[must_use]
    pub fn new(events: Vec<Event>) -> Option<Self> {
        let reply = events
            .iter()
            .find(|event| {
                event.channel == Channel::Discovery && event.direction == Direction::Received
            })
            .and_then(|event| SearchReply::decode(&event.bytes).ok())?;
        let address = Ipv4Addr::from(reply.address().octets());
        Some(Self {
            command: SocketAddrV4::new(address, reply.command_port().number()),
            control: SocketAddrV4::new(address, reply.control_port().number()),
            time: LocalTime::new(2026, 1, 1, 0, 0, 0)?,
            events,
        })
    }
}

impl Network for FixtureNetwork {
    type Error = ReplayError;
    type Discovery = DatagramReplay;
    type Keepalive = DatagramReplay;
    type Stream = Replay;
    type Trigger = ();
    type Clock = Pauses;

    fn local_address(&mut self, _: Ipv4Addr) -> Result<Ipv4Address, ReplayError> {
        Ok(Ipv4Address::new(HOST_ADDRESS))
    }

    fn open_discovery(&mut self) -> Result<DatagramReplay, ReplayError> {
        Ok(DatagramReplay::new(&self.events))
    }

    fn keepalive(&mut self, discovery: DatagramReplay) -> Result<DatagramReplay, ReplayError> {
        Ok(discovery)
    }

    fn open_trigger(&mut self, _: Ipv4Address) -> Result<((), Port), ReplayError> {
        Ok(((), Port::new(TRIGGER_PORT)))
    }

    fn connect(
        &mut self,
        endpoint: SocketAddrV4,
        connection: Connection,
    ) -> Result<Replay, ReplayError> {
        match connection {
            Connection::Command if endpoint == self.command => {
                Ok(Replay::new(&self.events, Channel::Command))
            }
            Connection::Control if endpoint == self.control => {
                Ok(Replay::new(&self.events, Channel::Control))
            }
            Connection::Command | Connection::Control => Err(ReplayError::UnknownEndpoint),
        }
    }

    fn local_time(&mut self) -> Result<LocalTime, ReplayError> {
        Ok(self.time)
    }

    fn clock(&mut self) -> Pauses {
        Pauses::default()
    }
}
