//! The retry rule follows section 5.1 of the ix1300-wifi protocol document: send each probe three times.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddrV4},
};

use vens_core::{Ipv4Address, MacAddress, discovery::Probe};
use vens_net::search::{Datagram, SearchError, search};

const TARGET: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::BROADCAST, 52217);
const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);

#[derive(Debug, PartialEq, Eq)]
struct Unplugged;

struct Script {
    sent: Vec<(Vec<u8>, SocketAddrV4)>,
    incoming: VecDeque<Option<Vec<u8>>>,
    unplugged: bool,
}

impl Script {
    fn new(incoming: impl IntoIterator<Item = Option<Vec<u8>>>) -> Self {
        Self {
            sent: Vec::new(),
            incoming: incoming.into_iter().collect(),
            unplugged: false,
        }
    }
}

impl Datagram for Script {
    type Error = Unplugged;

    fn send(&mut self, frame: &[u8], target: SocketAddrV4) -> Result<(), Unplugged> {
        if self.unplugged {
            return Err(Unplugged);
        }
        self.sent.push((frame.to_vec(), target));
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, Unplugged> {
        let Some(Some(datagram)) = self.incoming.pop_front() else {
            return Ok(None);
        };
        buffer
            .iter_mut()
            .zip(&datagram)
            .for_each(|(slot, byte)| *slot = *byte);
        Ok(Some(datagram.len().min(buffer.len())))
    }
}

fn reply() -> Vec<u8> {
    let mut frame = vec![0_u8; 144];
    let fields: [(usize, &[u8]); 3] = [
        (0, b"VENS"),
        (16, &[192, 0, 2, 20]),
        (104, b"ScanSnap iX1300"),
    ];
    for (offset, bytes) in fields {
        frame
            .iter_mut()
            .skip(offset)
            .zip(bytes)
            .for_each(|(slot, byte)| *slot = *byte);
    }
    frame
}

fn probe() -> Probe {
    Probe::search(HOST, HOST_MAC)
}

#[test]
fn the_first_valid_reply_ends_the_search() {
    let mut port = Script::new([Some(reply())]);
    let found = search(&mut port, probe(), TARGET);
    assert_eq!(
        found.map(|reply| reply.address()),
        Ok(Ipv4Address::new([192, 0, 2, 20]))
    );
    assert_eq!(port.sent, [(probe().encode().to_vec(), TARGET)]);
}

#[test]
fn a_silent_scanner_gets_three_probes_before_the_search_gives_up() {
    let mut port = Script::new([]);
    assert_eq!(
        search(&mut port, probe(), TARGET).err(),
        Some(SearchError::NoReply { attempts: 3 })
    );
    assert_eq!(port.sent.len(), 3);
}

#[test]
fn a_reply_to_a_later_probe_is_accepted() {
    let mut port = Script::new([None, Some(reply())]);
    assert!(search(&mut port, probe(), TARGET).is_ok());
    assert_eq!(port.sent.len(), 2);
}

#[test]
fn a_datagram_that_is_not_a_search_reply_is_skipped() {
    let mut port = Script::new([Some(b"noise".to_vec()), Some(reply())]);
    assert!(search(&mut port, probe(), TARGET).is_ok());
    assert_eq!(port.sent.len(), 1);
}

#[test]
fn a_port_failure_ends_the_search_with_its_cause() {
    let mut port = Script::new([]);
    port.unplugged = true;
    assert_eq!(
        search(&mut port, probe(), TARGET).err(),
        Some(SearchError::Port(Unplugged))
    );
}
