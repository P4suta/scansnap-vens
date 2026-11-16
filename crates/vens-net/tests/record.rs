//! A recorder logs every transfer that succeeds, so a hardware run leaves a trace without changing the protocol code.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddrV4},
    sync::mpsc,
};

use vens_net::{
    channel::Stream,
    record::Recorder,
    search::Datagram,
    trace::{Channel, Direction, Event},
};

#[derive(Debug, PartialEq, Eq)]
struct Closed;

struct Wire {
    incoming: VecDeque<u8>,
    outgoing: Vec<u8>,
}

impl Wire {
    fn new(incoming: &[u8]) -> Self {
        Self {
            incoming: incoming.iter().copied().collect(),
            outgoing: Vec::new(),
        }
    }
}

impl Stream for Wire {
    type Error = Closed;

    fn send(&mut self, bytes: &[u8]) -> Result<(), Closed> {
        self.outgoing.extend_from_slice(bytes);
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), Closed> {
        for slot in buffer {
            *slot = self.incoming.pop_front().ok_or(Closed)?;
        }
        Ok(())
    }
}

struct Air {
    incoming: VecDeque<Vec<u8>>,
}

impl Datagram for Air {
    type Error = Closed;

    fn send(&mut self, _: &[u8], _: SocketAddrV4) -> Result<(), Closed> {
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Closed> {
        Ok(self.incoming.pop_front().map(|datagram| {
            buffer
                .iter_mut()
                .zip(&datagram)
                .for_each(|(slot, byte)| *slot = *byte);
            datagram.len().min(buffer.len())
        }))
    }
}

fn event(channel: Channel, direction: Direction, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction,
        bytes: bytes.to_vec(),
    }
}

#[test]
fn recorders_log_each_successful_transfer_in_the_order_of_the_calls() {
    let (events, log) = mpsc::channel();
    let mut command = Recorder::new(Wire::new(&[1, 2, 3]), Channel::Command, events.clone());
    let mut control = Recorder::new(Wire::new(&[9]), Channel::Control, events);
    let mut two = [0; 2];
    let mut one = [0; 1];
    let outcome = (
        command.send(&[0xA0]),
        control.send(&[0xB0, 0xB1]),
        command.receive(&mut two),
        control.receive(&mut one),
        command.receive(&mut one),
    );
    assert_eq!(outcome, (Ok(()), Ok(()), Ok(()), Ok(()), Ok(())));
    assert_eq!(
        log.try_iter().collect::<Vec<_>>(),
        [
            event(Channel::Command, Direction::Sent, &[0xA0]),
            event(Channel::Control, Direction::Sent, &[0xB0, 0xB1]),
            event(Channel::Command, Direction::Received, &[1, 2]),
            event(Channel::Control, Direction::Received, &[9]),
            event(Channel::Command, Direction::Received, &[3]),
        ]
        .to_vec()
    );
}

#[test]
fn the_recorded_port_still_carries_the_bytes() {
    let (events, _log) = mpsc::channel();
    let mut command = Recorder::new(Wire::new(&[]), Channel::Command, events);
    let sent = command.send(&[1, 2]);
    assert_eq!(
        (sent, command.into_port().outgoing),
        (Ok(()), [1, 2].to_vec())
    );
}

#[test]
fn a_failed_or_empty_transfer_is_not_logged() {
    let (events, log) = mpsc::channel();
    let mut command = Recorder::new(Wire::new(&[7]), Channel::Command, events);
    let mut two = [0; 2];
    let outcome = (
        command.send(&[]),
        command.receive(&mut []),
        command.receive(&mut two),
    );
    assert_eq!(
        (outcome, log.try_iter().count()),
        ((Ok(()), Ok(()), Err(Closed)), 0)
    );
}

#[test]
fn recording_without_a_listener_leaves_the_transfer_intact() {
    let (events, log) = mpsc::channel();
    drop(log);
    let mut command = Recorder::new(Wire::new(&[5]), Channel::Command, events);
    let mut one = [0; 1];
    assert_eq!(
        (command.send(&[1]), command.receive(&mut one), one),
        (Ok(()), Ok(()), [5])
    );
}

#[test]
fn a_datagram_recorder_logs_sent_frames_and_received_datagrams() {
    let (events, log) = mpsc::channel();
    let target = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 7), 52217);
    let air = Air {
        incoming: [[0xC0, 0xC1].to_vec()].into_iter().collect(),
    };
    let mut discovery = Recorder::new(air, Channel::Discovery, events);
    let mut buffer = [0; 8];
    let outcome = (
        discovery.send(&[0x56], target),
        discovery.receive(&mut buffer),
        discovery.receive(&mut buffer),
    );
    assert_eq!(outcome, (Ok(()), Ok(Some(2)), Ok(None)));
    assert_eq!(
        log.try_iter().collect::<Vec<_>>(),
        [
            event(Channel::Discovery, Direction::Sent, &[0x56]),
            event(Channel::Discovery, Direction::Received, &[0xC0, 0xC1]),
        ]
        .to_vec()
    );
}
