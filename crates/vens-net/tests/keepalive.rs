//! The keepalive driver sends on the schedule of the core and reads the answers without blocking a job.
//! The packets and the answers follow sections 5.1 and 5.3 of the ix1300-wifi protocol document.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddrV4},
};

use vens_core::{Ipv4Address, MacAddress, discovery::Probe, keepalive::Elapsed};
use vens_net::{
    keepalive::{DatagramKeepalive, Keepalive},
    search::Datagram,
};

const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 12]);
const MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);

#[derive(Debug, PartialEq, Eq)]
struct Down;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scanner {
    Answers,
    Silent,
    SendsSearchReplies,
    Unreachable,
}

struct Air {
    scanner: Scanner,
    inbox: VecDeque<Vec<u8>>,
    sent: Vec<Vec<u8>>,
}

impl Air {
    const fn new(scanner: Scanner) -> Self {
        Self {
            scanner,
            inbox: VecDeque::new(),
            sent: Vec::new(),
        }
    }
}

fn answer() -> Vec<u8> {
    let mut frame = b"VENS".to_vec();
    frame.extend_from_slice(&[0x80, 0x01]);
    frame.resize(12, 0);
    frame
}

fn search_reply() -> Vec<u8> {
    let mut frame = b"VENS".to_vec();
    frame.resize(144, 0);
    frame
}

impl Datagram for Air {
    type Error = Down;

    fn send(&mut self, frame: &[u8], _: SocketAddrV4) -> Result<(), Down> {
        match self.scanner {
            Scanner::Unreachable => return Err(Down),
            Scanner::Answers => self.inbox.push_back(answer()),
            Scanner::SendsSearchReplies => self.inbox.push_back(search_reply()),
            Scanner::Silent => {}
        }
        self.sent.push(frame.to_vec());
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

fn target() -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 7), 52217)
}

fn driven(scanner: Scanner, ticks: &[u64]) -> (Vec<Vec<u8>>, u32, Option<Down>) {
    let mut keepalive = DatagramKeepalive::new(
        Air::new(scanner),
        Probe::keepalive(HOST, MAC),
        target(),
        Elapsed::from_millis(0),
    );
    for millis in ticks {
        keepalive.tick(Elapsed::from_millis(*millis));
    }
    let replies = keepalive.replies();
    let (port, failure) = keepalive.into_parts();
    (port.sent, replies, failure)
}

fn probes(count: usize) -> Vec<Vec<u8>> {
    std::iter::repeat_with(|| Probe::keepalive(HOST, MAC).encode().to_vec())
        .take(count)
        .collect()
}

#[test]
fn a_silent_scanner_gets_three_keepalive_probes_in_a_round() {
    assert_eq!(
        driven(Scanner::Silent, &[0, 100, 500, 1000, 1500, 14_999]),
        (probes(3), 0, None)
    );
}

#[test]
fn an_answer_ends_the_round_until_the_next_one() {
    assert_eq!(
        driven(Scanner::Answers, &[0, 500, 1000, 15_000, 15_500]),
        (probes(2), 2, None)
    );
}

#[test]
fn a_search_reply_is_not_an_answer() {
    assert_eq!(
        driven(Scanner::SendsSearchReplies, &[0, 500, 1000]),
        (probes(3), 0, None)
    );
}

#[test]
fn a_send_failure_is_kept_and_never_stops_later_ticks() {
    assert_eq!(
        driven(Scanner::Unreachable, &[0, 500, 1000]),
        (Vec::new(), 0, Some(Down))
    );
}
