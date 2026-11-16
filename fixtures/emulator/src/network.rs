use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::mpsc,
};

use vens_core::{Ipv4Address, Port, control::LocalTime, discovery::SearchReply};
use vens_net::{
    network::{Connection, Network},
    search::Datagram,
    trace::{Channel, Direction, Event},
};

use crate::{
    redact::{HOST_ADDRESS, TRIGGER_PORT},
    replay::{DatagramReplay, Replay, ReplayError},
    session::Pauses,
};

/// Names a place that a command reached through the fixture network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    Datagram(SocketAddrV4),
    Connect(SocketAddrV4, Connection),
}

/// Plays the recorded discovery datagrams back and reports the target of each one that the host sends.
#[derive(Debug)]
pub struct ObservedDatagram {
    replay: DatagramReplay,
    contacts: Option<mpsc::Sender<Contact>>,
}

impl ObservedDatagram {
    #[must_use]
    pub fn finished(&self) -> bool {
        self.replay.finished()
    }
}

fn report(contacts: Option<&mpsc::Sender<Contact>>, contact: Contact) {
    if let Some(contacts) = contacts {
        let _ = contacts.send(contact);
    }
}

impl Datagram for ObservedDatagram {
    type Error = ReplayError;

    fn send(&mut self, frame: &[u8], target: SocketAddrV4) -> Result<(), ReplayError> {
        report(self.contacts.as_ref(), Contact::Datagram(target));
        self.replay.send(frame, target)
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, ReplayError> {
        self.replay.receive(buffer)
    }
}

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
    contacts: Option<mpsc::Sender<Contact>>,
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
            contacts: None,
        })
    }

    /// Builds the network together with a receiver of every datagram target and connection that a command reaches.
    #[must_use]
    pub fn observed(events: Vec<Event>) -> Option<(Self, mpsc::Receiver<Contact>)> {
        let (contacts, log) = mpsc::channel();
        let network = Self::new(events)?;
        Some((
            Self {
                contacts: Some(contacts),
                ..network
            },
            log,
        ))
    }
}

impl Network for FixtureNetwork {
    type Error = ReplayError;
    type Discovery = ObservedDatagram;
    type Keepalive = ObservedDatagram;
    type Stream = Replay;
    type Trigger = ();
    type Clock = Pauses;

    fn local_address(&mut self, _: Ipv4Addr) -> Result<Ipv4Address, ReplayError> {
        Ok(Ipv4Address::new(HOST_ADDRESS))
    }

    fn open_discovery(&mut self) -> Result<ObservedDatagram, ReplayError> {
        Ok(ObservedDatagram {
            replay: DatagramReplay::new(&self.events),
            contacts: self.contacts.clone(),
        })
    }

    fn keepalive(&mut self, discovery: ObservedDatagram) -> Result<ObservedDatagram, ReplayError> {
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
        report(
            self.contacts.as_ref(),
            Contact::Connect(endpoint, connection),
        );
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
