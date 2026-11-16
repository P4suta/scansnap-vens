//! The scan operation as separate steps: open the connections, read the status, reserve, and scan with a release.
//! The bridge answers a job request between the steps, so each step runs on its own recorded replies.

use std::{convert::Infallible, net::Ipv4Addr};

use vens_core::{control::ReserveRefusal, scan::Side, status::PaperPath};
use vens_emulator::{network::FixtureNetwork, replay::ReplayError};
use vens_net::{
    control::ControlError,
    operation::{Scanned, open},
    scan::PageSink,
    trace::{self, Event},
};

const ONE_SHEET: &str = include_str!("../traces/one-duplex-sheet.trace");
const THREE_SHEETS: &str = include_str!("../traces/three-duplex-sheets.trace");
const EMPTY_CHUTE: &str = include_str!("../traces/empty-chute.trace");
const RESERVE_REPLY: usize = 8;
const RESULT: usize = 8;

#[derive(Default)]
struct Sides(Vec<(u32, Side)>);

impl PageSink for Sides {
    type Error = Infallible;

    fn append(&mut self, _: u32, _: Side, _: &[u8]) -> Result<(), Infallible> {
        Ok(())
    }

    fn finish(&mut self, sheet: u32, side: Side, _: u16) -> Result<(), Infallible> {
        self.0.push((sheet, side));
        Ok(())
    }
}

fn events(fixture: &str) -> Vec<Event> {
    trace::parse(fixture).unwrap_or_default()
}

/// Lists both sides of each sheet, numbered from zero as the job counts them.
fn duplex(sheets: u32) -> Vec<(u32, Side)> {
    (0..sheets)
        .flat_map(|sheet| [(sheet, Side::Front), (sheet, Side::Back)])
        .collect()
}

type Stepped = (Option<Scanned>, Option<ReplayError>, Vec<(u32, Side)>);

fn stepped(events: Vec<Event>) -> Option<Stepped> {
    let mut network = FixtureNetwork::new(events)?;
    let mut connected = open(&mut network, Ipv4Addr::BROADCAST).ok()?;
    let prepared = connected.prepare().ok()?;
    let reserved = connected.reserve(prepared).ok()?;
    let mut sides = Sides::default();
    let run = reserved.scan(&mut sides);
    Some((run.outcome.ok(), run.keepalive_failure, sides.0))
}

#[test]
fn the_steps_scan_a_recorded_stack_and_count_its_keepalive_replies() {
    assert_eq!(
        stepped(events(THREE_SHEETS)),
        Some((
            Some(Scanned {
                sheets: 3,
                keepalive_replies: 3
            }),
            None,
            duplex(3)
        ))
    );
}

#[test]
fn the_steps_scan_a_recorded_sheet() {
    assert_eq!(
        stepped(events(ONE_SHEET)).map(|(scanned, _, sides)| (scanned, sides)),
        Some((
            Some(Scanned {
                sheets: 1,
                keepalive_replies: 0
            }),
            duplex(1)
        ))
    );
}

#[test]
fn the_status_step_reads_an_empty_chute_before_any_reserve() {
    let paper = FixtureNetwork::new(events(EMPTY_CHUTE)).and_then(|mut network| {
        let mut connected = open(&mut network, Ipv4Addr::BROADCAST).ok()?;
        connected.prepare().ok().map(|prepared| prepared.paper())
    });
    assert_eq!(paper, Some(PaperPath::Absent));
}

#[test]
fn a_refused_reserve_returns_the_refusal() {
    let mut occupied = events(ONE_SHEET);
    if let Some(reply) = occupied.get_mut(RESERVE_REPLY) {
        reply
            .bytes
            .iter_mut()
            .skip(RESULT)
            .zip((-4_i32).to_be_bytes())
            .for_each(|(slot, byte)| *slot = byte);
    }
    let refusal = FixtureNetwork::new(occupied).and_then(|mut network| {
        let mut connected = open(&mut network, Ipv4Addr::BROADCAST).ok()?;
        let prepared = connected.prepare().ok()?;
        connected.reserve(prepared).err()
    });
    assert_eq!(
        refusal,
        Some(ControlError::Reserve(ReserveRefusal::Occupied))
    );
}
