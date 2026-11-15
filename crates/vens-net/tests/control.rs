//! The exchange follows sections 4, 5.3, and 5.4 of the ix1300-wifi protocol document.

use std::collections::VecDeque;

use vens_core::{
    HostId, Ipv4Address, MacAddress, Port,
    control::{LocalTime, Release, Reserve, ReserveMode, ReserveRefusal, UtcOffset},
};
use vens_net::{
    channel::Stream,
    control::{ControlChannel, ControlError},
};

const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);

#[derive(Debug, PartialEq, Eq)]
struct Closed;

struct Wire {
    incoming: VecDeque<u8>,
    outgoing: Vec<u8>,
}

impl Wire {
    fn new(incoming: &[&[u8]]) -> Self {
        Self {
            incoming: incoming.iter().copied().flatten().copied().collect(),
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

fn greeting() -> Vec<u8> {
    [0x10_u32.to_be_bytes(), *b"VENS", [0; 4], [0; 4]].concat()
}

fn reply(length: usize, result: i32) -> Vec<u8> {
    let mut frame = [[0; 4], *b"VENS", result.to_be_bytes()].concat();
    frame.resize(length, 0);
    frame
}

fn reserve() -> Option<Reserve> {
    Some(Reserve {
        mode: ReserveMode::Scan,
        mac: HOST_MAC,
        host: Ipv4Address::new([192, 0, 2, 10]),
        trigger_port: Port::new(55265),
        time: LocalTime::from_unix_seconds(1_700_000_000)?,
        host_id: HostId::new([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]),
        offset: UtcOffset::from_seconds(0),
    })
}

#[test]
fn a_reserve_and_a_release_cross_one_connection_after_the_greeting() {
    let wire = Wire::new(&[&greeting(), &reply(20, 0), &reply(16, 0)]);
    let outcome = reserve().map(|reserve| {
        ControlChannel::open(wire).map(|mut channel| {
            let reserved = channel.reserve(reserve);
            let released = channel.release(Release::new(HOST_MAC));
            (reserved, released, channel.into_stream().outgoing)
        })
    });
    let expected = reserve().map(|reserve| {
        [
            reserve.encode().as_slice(),
            Release::new(HOST_MAC).encode().as_slice(),
        ]
        .concat()
    });
    assert_eq!(outcome, expected.map(|sent| Ok((Ok(()), Ok(()), sent))));
}

#[test]
fn a_refused_reserve_reports_the_scanner_reason() {
    let wire = Wire::new(&[&greeting(), &reply(20, -6)]);
    let outcome = reserve()
        .map(|reserve| ControlChannel::open(wire).map(|mut channel| channel.reserve(reserve)));
    assert_eq!(
        outcome,
        Some(Ok(Err(ControlError::Reserve(
            ReserveRefusal::HostIdNotRegistered
        ))))
    );
}

#[test]
fn a_refused_release_reports_the_result_code() {
    let wire = Wire::new(&[&greeting(), &reply(16, -1)]);
    let outcome =
        ControlChannel::open(wire).map(|mut channel| channel.release(Release::new(HOST_MAC)));
    assert_eq!(outcome, Ok(Err(ControlError::Release(-1))));
}

#[test]
fn a_connection_that_closes_before_the_reply_reports_the_port_failure() {
    let wire = Wire::new(&[&greeting()]);
    let outcome =
        ControlChannel::open(wire).map(|mut channel| channel.release(Release::new(HOST_MAC)));
    assert_eq!(outcome, Ok(Err(ControlError::Port(Closed))));
}
