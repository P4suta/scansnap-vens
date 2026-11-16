//! A replay plays one recorded channel back and refuses any host transfer that leaves the recording.

use std::net::{Ipv4Addr, SocketAddrV4};

use vens_emulator::replay::{DatagramReplay, Replay, ReplayError};
use vens_net::{
    channel::Stream,
    search::Datagram,
    trace::{Channel, Direction, Event},
};

fn sent(channel: Channel, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction: Direction::Sent,
        bytes: bytes.to_vec(),
    }
}

fn received(channel: Channel, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction: Direction::Received,
        bytes: bytes.to_vec(),
    }
}

fn target() -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 7), 52217)
}

#[test]
fn a_replay_serves_the_recorded_replies_after_the_recorded_requests_of_its_channel() {
    let events = [
        received(Channel::Command, &[0x10]),
        received(Channel::Control, &[0x55]),
        sent(Channel::Command, &[1, 2]),
        sent(Channel::Command, &[3]),
        received(Channel::Command, &[7, 8, 9]),
    ];
    let mut replay = Replay::new(&events, Channel::Command);
    let mut greeting = [0; 1];
    let mut answer = [0; 3];
    let outcome = (
        replay.receive(&mut greeting),
        replay.send(&[1]),
        replay.send(&[2, 3]),
        replay.receive(&mut answer),
    );
    assert_eq!(
        (outcome, greeting, answer, replay.finished()),
        ((Ok(()), Ok(()), Ok(()), Ok(())), [0x10], [7, 8, 9], true)
    );
}

#[test]
fn a_request_byte_that_differs_from_the_recording_is_refused_with_its_position() {
    let events = [
        sent(Channel::Command, &[1, 2, 3]),
        received(Channel::Command, &[9]),
    ];
    let mut replay = Replay::new(&events, Channel::Command);
    assert_eq!(
        replay.send(&[1, 5, 3]),
        Err(ReplayError::Diverged {
            offset: 1,
            expected: 2,
            actual: 5
        })
    );
}

#[test]
fn a_read_before_the_recorded_request_is_complete_is_refused() {
    let events = [
        sent(Channel::Control, &[1, 2]),
        received(Channel::Control, &[9]),
    ];
    let mut replay = Replay::new(&events, Channel::Control);
    let mut answer = [0; 1];
    assert_eq!(
        (replay.send(&[1]), replay.receive(&mut answer)),
        (Ok(()), Err(ReplayError::OutOfOrder { offset: 1 }))
    );
}

#[test]
fn a_request_while_a_reply_is_due_is_refused() {
    let events = [
        sent(Channel::Command, &[1]),
        received(Channel::Command, &[9]),
    ];
    let mut replay = Replay::new(&events, Channel::Command);
    assert_eq!(
        (replay.send(&[1]), replay.send(&[2])),
        (Ok(()), Err(ReplayError::OutOfOrder { offset: 1 }))
    );
}

#[test]
fn a_read_beyond_the_recording_is_refused() {
    let events = [received(Channel::Command, &[9])];
    let mut replay = Replay::new(&events, Channel::Command);
    let mut answer = [0; 2];
    assert_eq!(replay.receive(&mut answer), Err(ReplayError::Exhausted));
}

#[test]
fn a_replay_with_unsent_requests_is_unfinished() {
    let events = [sent(Channel::Command, &[1])];
    assert!(!Replay::new(&events, Channel::Command).finished());
}

#[test]
fn a_datagram_replay_answers_the_recorded_probe_with_the_recorded_reply() {
    let events = [
        sent(Channel::Discovery, &[0x56]),
        received(Channel::Command, &[0x10]),
        received(Channel::Discovery, &[0xAA, 0xBB]),
    ];
    let mut replay = DatagramReplay::new(&events);
    let mut buffer = [0; 4];
    let outcome = (
        replay.send(&[0x56], target()),
        replay.receive(&mut buffer),
        replay.receive(&mut buffer),
    );
    assert_eq!(
        (outcome, buffer, replay.finished()),
        ((Ok(()), Ok(Some(2)), Ok(None)), [0xAA, 0xBB, 0, 0], true)
    );
}

#[test]
fn a_datagram_that_differs_from_the_recording_is_refused() {
    let events = [
        sent(Channel::Discovery, &[0x56, 0x45]),
        received(Channel::Discovery, &[0xAA]),
    ];
    let mut replay = DatagramReplay::new(&events);
    assert_eq!(
        (
            replay.send(&[0x56, 0x46], target()),
            replay.send(&[0x56], target())
        ),
        (
            Err(ReplayError::Diverged {
                offset: 1,
                expected: 0x45,
                actual: 0x46
            }),
            Err(ReplayError::DatagramLength {
                expected: 2,
                actual: 1
            })
        )
    );
}

#[test]
fn a_datagram_beyond_the_recording_is_refused() {
    let mut replay = DatagramReplay::new(&[]);
    assert_eq!(replay.send(&[0x56], target()), Err(ReplayError::Exhausted));
}
