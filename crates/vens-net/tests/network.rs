//! The network port gives a command every host facility it needs, and the recording network logs each transfer in call order.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddrV4},
    sync::mpsc,
};

use vens_core::{Ipv4Address, Port, control::LocalTime, job::Pause, keepalive::Elapsed};
use vens_net::{
    channel::Stream,
    network::{Connection, Network, RecordingNetwork},
    scan::Clock,
    search::Datagram,
    trace::{Channel, Direction, Event},
};

#[derive(Debug, PartialEq, Eq)]
struct Down;

#[derive(Default)]
struct Air {
    inbox: VecDeque<Vec<u8>>,
    nonblocking: bool,
}

impl Datagram for Air {
    type Error = Down;

    fn send(&mut self, _: &[u8], _: SocketAddrV4) -> Result<(), Down> {
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Down> {
        Ok(self.inbox.pop_front().map(|datagram| {
            buffer
                .iter_mut()
                .zip(&datagram)
                .for_each(|(slot, byte)| *slot = *byte);
            datagram.len().min(buffer.len())
        }))
    }
}

struct Wire(VecDeque<u8>);

impl Stream for Wire {
    type Error = Down;

    fn send(&mut self, _: &[u8]) -> Result<(), Down> {
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), Down> {
        for slot in buffer {
            *slot = self.0.pop_front().ok_or(Down)?;
        }
        Ok(())
    }
}

struct Frozen;

impl Clock for Frozen {
    fn wait(&mut self, _: Pause) {}

    fn now(&mut self) -> Elapsed {
        Elapsed::from_millis(0)
    }
}

struct Scripted {
    connections: Vec<(SocketAddrV4, Connection)>,
    keepalive_scanner: Option<Ipv4Addr>,
}

impl Network for Scripted {
    type Error = Down;
    type Discovery = Air;
    type Keepalive = Air;
    type Stream = Wire;
    type Trigger = ();
    type Clock = Frozen;

    fn local_address(&mut self, _: Ipv4Addr) -> Result<Ipv4Address, Down> {
        Ok(Ipv4Address::new([192, 0, 2, 12]))
    }

    fn open_discovery(&mut self) -> Result<Air, Down> {
        Ok(Air {
            inbox: [[1, 2].to_vec()].into_iter().collect(),
            nonblocking: false,
        })
    }

    fn keepalive(&mut self, discovery: Air, scanner: Ipv4Addr) -> Result<Air, Down> {
        self.keepalive_scanner = Some(scanner);
        Ok(Air {
            nonblocking: true,
            ..discovery
        })
    }

    fn open_trigger(&mut self, _: Ipv4Address) -> Result<((), Port), Down> {
        Ok(((), Port::new(55265)))
    }

    fn connect(&mut self, endpoint: SocketAddrV4, connection: Connection) -> Result<Wire, Down> {
        self.connections.push((endpoint, connection));
        Ok(Wire([7, 8, 9].into_iter().collect()))
    }

    fn local_time(&mut self) -> Result<LocalTime, Down> {
        LocalTime::new(2026, 1, 1, 0, 0, 0).ok_or(Down)
    }

    fn clock(&mut self) -> Frozen {
        Frozen
    }
}

fn event(channel: Channel, direction: Direction, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction,
        bytes: bytes.to_vec(),
    }
}

fn endpoint(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 7), port)
}

type Observed = (
    Result<(), Down>,
    Option<bool>,
    Option<Ipv4Addr>,
    Vec<(SocketAddrV4, Connection)>,
    Vec<Event>,
);

fn recorded_run() -> Observed {
    let (events, log) = mpsc::channel();
    let mut network = RecordingNetwork::new(
        Scripted {
            connections: Vec::new(),
            keepalive_scanner: None,
        },
        events,
    );
    let mut buffer = [0; 4];
    let mut one = [0; 1];
    let mut two = [0; 2];
    let outcome = (|| {
        let mut discovery = network.open_discovery()?;
        discovery.send(&[0x56], endpoint(52217))?;
        discovery.receive(&mut buffer)?;
        let mut command = network.connect(endpoint(53218), Connection::Command)?;
        let mut control = network.connect(endpoint(53219), Connection::Control)?;
        command.receive(&mut one)?;
        control.receive(&mut two)?;
        let keepalive = network.keepalive(discovery, *endpoint(52217).ip())?;
        Ok(Some(keepalive.into_port().nonblocking))
    })();
    let nonblocking = outcome.as_ref().ok().copied().flatten();
    let scripted = network.into_inner();
    (
        outcome.map(|_| ()),
        nonblocking,
        scripted.keepalive_scanner,
        scripted.connections,
        log.try_iter().collect(),
    )
}

#[test]
fn a_recording_network_logs_every_transfer_with_its_channel_in_call_order() {
    let (outcome, nonblocking, keepalive_scanner, connections, events) = recorded_run();
    assert_eq!(
        (outcome, nonblocking, keepalive_scanner, connections, events),
        (
            Ok(()),
            Some(true),
            Some(*endpoint(52217).ip()),
            [
                (endpoint(53218), Connection::Command),
                (endpoint(53219), Connection::Control),
            ]
            .to_vec(),
            [
                event(Channel::Discovery, Direction::Sent, &[0x56]),
                event(Channel::Discovery, Direction::Received, &[1, 2]),
                event(Channel::Command, Direction::Received, &[7]),
                event(Channel::Control, Direction::Received, &[7, 8]),
            ]
            .to_vec()
        )
    );
}
