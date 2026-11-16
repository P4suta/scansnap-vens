use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::mpsc,
};

use vens_core::{Ipv4Address, Port, control::LocalTime};

use crate::{
    channel::Stream,
    record::Recorder,
    scan::Clock,
    search::Datagram,
    trace::{Channel, Event},
};

/// Names the TCP connection that a command opens to the scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Command,
    Control,
}

impl Connection {
    const fn channel(self) -> Channel {
        match self {
            Self::Command => Channel::Command,
            Self::Control => Channel::Control,
        }
    }
}

/// Gives a command every host facility it needs to reach a scanner, so tests can supply recorded ones.
pub trait Network {
    type Error;
    type Discovery: Datagram<Error = Self::Error>;
    type Keepalive: Datagram<Error = Self::Error>;
    type Stream: Stream<Error = Self::Error>;
    /// Keeps the button event port open while it lives.
    type Trigger;
    type Clock: Clock;

    /// Names the local address that the host uses toward `target`.
    ///
    /// # Errors
    ///
    /// Fails when the host has no route toward `target`.
    fn local_address(&mut self, target: Ipv4Addr) -> Result<Ipv4Address, Self::Error>;

    /// # Errors
    ///
    /// Fails when the discovery port can't open.
    fn open_discovery(&mut self) -> Result<Self::Discovery, Self::Error>;

    /// Turns the discovery port into the keepalive port, whose receive returns at once.
    ///
    /// # Errors
    ///
    /// Fails when the port can't change its mode.
    fn keepalive(&mut self, discovery: Self::Discovery) -> Result<Self::Keepalive, Self::Error>;

    /// Opens the port for button events on `local` and returns its number.
    ///
    /// # Errors
    ///
    /// Fails when the port can't open.
    fn open_trigger(&mut self, local: Ipv4Address) -> Result<(Self::Trigger, Port), Self::Error>;

    /// # Errors
    ///
    /// Fails when the scanner doesn't accept the connection.
    fn connect(
        &mut self,
        endpoint: SocketAddrV4,
        connection: Connection,
    ) -> Result<Self::Stream, Self::Error>;

    /// Reads the wall clock for the reserve.
    ///
    /// # Errors
    ///
    /// Fails when the clock lies outside the range of a reserve time.
    fn local_time(&mut self) -> Result<LocalTime, Self::Error>;

    fn clock(&mut self) -> Self::Clock;
}

/// Wraps another network and logs every transfer of its ports as a trace event.
#[derive(Debug)]
pub struct RecordingNetwork<N> {
    network: N,
    events: mpsc::Sender<Event>,
}

impl<N> RecordingNetwork<N> {
    #[must_use]
    pub const fn new(network: N, events: mpsc::Sender<Event>) -> Self {
        Self { network, events }
    }

    #[must_use]
    pub fn into_inner(self) -> N {
        self.network
    }
}

impl<N: Network> Network for RecordingNetwork<N> {
    type Error = N::Error;
    type Discovery = Recorder<N::Discovery>;
    type Keepalive = Recorder<N::Keepalive>;
    type Stream = Recorder<N::Stream>;
    type Trigger = N::Trigger;
    type Clock = N::Clock;

    fn local_address(&mut self, target: Ipv4Addr) -> Result<Ipv4Address, N::Error> {
        self.network.local_address(target)
    }

    fn open_discovery(&mut self) -> Result<Self::Discovery, N::Error> {
        let port = self.network.open_discovery()?;
        Ok(Recorder::new(port, Channel::Discovery, self.events.clone()))
    }

    fn keepalive(&mut self, discovery: Self::Discovery) -> Result<Self::Keepalive, N::Error> {
        let port = self.network.keepalive(discovery.into_port())?;
        Ok(Recorder::new(port, Channel::Discovery, self.events.clone()))
    }

    fn open_trigger(&mut self, local: Ipv4Address) -> Result<(N::Trigger, Port), N::Error> {
        self.network.open_trigger(local)
    }

    fn connect(
        &mut self,
        endpoint: SocketAddrV4,
        connection: Connection,
    ) -> Result<Self::Stream, N::Error> {
        let stream = self.network.connect(endpoint, connection)?;
        Ok(Recorder::new(
            stream,
            connection.channel(),
            self.events.clone(),
        ))
    }

    fn local_time(&mut self) -> Result<LocalTime, N::Error> {
        self.network.local_time()
    }

    fn clock(&mut self) -> N::Clock {
        self.network.clock()
    }
}
