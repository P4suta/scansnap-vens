//! The job thread runs one scan and reports the reserve, each page, and the end to the owner.
//! A refusal before the reserve frees the job request at once, and the scanner stays free for its other hosts.
//! The occupied, unregistered, jammed, and silent cases change recorded replies, because the scanner never showed them during the recordings.

use std::net::Ipv4Addr;

use vens_core::jpeg::frame_size;
use vens_daemon::job::{JobEvent, Report, run_job};
use vens_emulator::network::FixtureNetwork;
use vens_escl::bridge::{Ending, Fault, Refusal, Side};
use vens_net::trace::{self, Channel, Event};

const THREE_SHEETS: &str =
    include_str!("../../../fixtures/emulator/traces/three-duplex-sheets.trace");
const ONE_SHEET: &str = include_str!("../../../fixtures/emulator/traces/one-duplex-sheet.trace");
const EMPTY_CHUTE: &str = include_str!("../../../fixtures/emulator/traces/empty-chute.trace");
const STATUS_REPLY: usize = 6;
const RESERVE_REPLY: usize = 8;
const LAST_SENSE: usize = 158;
const RELEASE_REPLY: usize = 162;
const HOST_ID: usize = 0x20;
const RESULT: usize = 8;
const QUALIFIER: usize = 13;

#[derive(Default)]
struct Reports(Vec<JobEvent>);

impl Report for Reports {
    fn report(&mut self, event: JobEvent) {
        self.0.push(event);
    }
}

/// An event with each page reduced to whether its image has a readable frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    Reserved,
    Refused(Refusal),
    Page(Side, bool),
    Finished(Ending, bool),
}

fn events(fixture: &str) -> Vec<Event> {
    trace::parse(fixture).unwrap_or_default()
}

fn changed(fixture: &str, index: usize, offset: usize, bytes: &[u8]) -> Vec<Event> {
    let mut events = events(fixture);
    if let Some(event) = events.get_mut(index) {
        event
            .bytes
            .iter_mut()
            .skip(offset)
            .zip(bytes)
            .for_each(|(slot, byte)| *slot = *byte);
    }
    events
}

fn silent() -> Vec<Event> {
    let recorded = events(THREE_SHEETS);
    let mut discovery = recorded
        .iter()
        .filter(|event| event.channel == Channel::Discovery)
        .take(2);
    let probe = discovery.next().cloned();
    let reply = discovery.next().cloned();
    std::iter::repeat_n(probe, 21)
        .chain([reply])
        .flatten()
        .collect()
}

fn seen(events: Vec<Event>) -> Option<Vec<Seen>> {
    let mut network = FixtureNetwork::new(events)?;
    let mut reports = Reports::default();
    run_job(&mut network, Ipv4Addr::BROADCAST, &mut reports);
    Some(
        reports
            .0
            .into_iter()
            .map(|event| match event {
                JobEvent::Reserved => Seen::Reserved,
                JobEvent::Refused { refusal, .. } => Seen::Refused(refusal),
                JobEvent::Page { side, page } => Seen::Page(side, frame_size(&page).is_ok()),
                JobEvent::Finished { ending, detail } => Seen::Finished(ending, detail.is_some()),
            })
            .collect(),
    )
}

fn duplex(sheets: usize) -> Vec<Seen> {
    std::iter::repeat_n(
        [Seen::Page(Side::Front, true), Seen::Page(Side::Back, true)],
        sheets,
    )
    .flatten()
    .collect()
}

fn scanned(sheets: usize, ending: Seen) -> Vec<Seen> {
    [[Seen::Reserved].to_vec(), duplex(sheets), [ending].to_vec()].concat()
}

#[test]
fn a_recorded_stack_reports_the_reserve_each_page_and_the_end() {
    assert_eq!(
        seen(events(THREE_SHEETS)),
        Some(scanned(3, Seen::Finished(Ending::Completed, false)))
    );
}

#[test]
fn a_keepalive_failure_completes_the_job_with_a_detail() {
    assert_eq!(
        seen(events(ONE_SHEET)),
        Some(scanned(1, Seen::Finished(Ending::Completed, true)))
    );
}

#[test]
fn an_empty_chute_is_refused_before_the_reserve() {
    assert_eq!(
        seen(events(EMPTY_CHUTE)),
        Some([Seen::Refused(Refusal::NoPaper)].to_vec())
    );
}

#[test]
fn a_scanner_held_by_another_host_is_busy() {
    assert_eq!(
        seen(changed(
            ONE_SHEET,
            RESERVE_REPLY,
            RESULT,
            &(-4_i32).to_be_bytes()
        )),
        Some([Seen::Refused(Refusal::Busy)].to_vec())
    );
}

#[test]
fn a_silent_or_unregistered_scanner_is_unreachable() {
    assert_eq!(
        [
            seen(silent()),
            seen(changed(THREE_SHEETS, STATUS_REPLY, HOST_ID, &[0xFF; 8])),
        ],
        [
            Some([Seen::Refused(Refusal::Unreachable)].to_vec()),
            Some([Seen::Refused(Refusal::Unreachable)].to_vec()),
        ]
    );
}

#[test]
fn a_jam_ends_the_job_with_its_fault_after_the_scanned_pages() {
    assert_eq!(
        seen(changed(THREE_SHEETS, LAST_SENSE, QUALIFIER, &[0x01])),
        Some(scanned(3, Seen::Finished(Ending::Failed(Fault::Jam), true)))
    );
}

#[test]
fn a_refused_release_after_a_good_scan_keeps_the_pages_and_names_the_refusal() {
    assert_eq!(
        seen(changed(
            THREE_SHEETS,
            RELEASE_REPLY,
            RESULT,
            &(-1_i32).to_be_bytes()
        )),
        Some(scanned(3, Seen::Finished(Ending::Completed, true)))
    );
}
