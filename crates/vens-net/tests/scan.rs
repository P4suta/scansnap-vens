//! Drives the pure job over a scripted stream with the replies of section 8.1 of the ix1300-wifi protocol document.

use std::collections::VecDeque;

use vens_core::{
    Ipv4Address, MacAddress,
    job::{JobError, Pause, Summary},
    scan::Side,
    sense::Condition,
};
use vens_net::{
    channel::{ChannelError, CommandChannel, Stream},
    scan::{Clock, PageSink, ScanError, run},
};

const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);
const GOOD: u32 = 0;
const CHECK: u32 = 2;
const END_OF_SIDE: [u8; 3] = [0x40, 0, 0];
const NO_DATA_YET: [u8; 3] = [3, 0x80, 0x13];
const NO_PAPER: [u8; 3] = [3, 0x80, 0x03];
const JAM: [u8; 3] = [3, 0x80, 0x01];

#[derive(Debug, PartialEq, Eq)]
struct Closed;

#[derive(Debug, PartialEq, Eq)]
struct Full;

struct Wire(VecDeque<u8>);

impl Stream for Wire {
    type Error = Closed;

    fn send(&mut self, _: &[u8]) -> Result<(), Closed> {
        Ok(())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), Closed> {
        for slot in buffer {
            *slot = self.0.pop_front().ok_or(Closed)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Pauses(Vec<Pause>);

impl Clock for Pauses {
    fn wait(&mut self, pause: Pause) {
        self.0.push(pause);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    Append(u32, Side, Vec<u8>),
    Finish(u32, Side, u16),
}

#[derive(Default)]
struct Pages {
    seen: Vec<Page>,
    capacity: Option<usize>,
}

impl PageSink for Pages {
    type Error = Full;

    fn append(&mut self, sheet: u32, side: Side, data: &[u8]) -> Result<(), Full> {
        let appended = self
            .seen
            .iter()
            .filter(|page| matches!(page, Page::Append(..)))
            .count();
        if self.capacity == Some(appended) {
            return Err(Full);
        }
        self.seen.push(Page::Append(sheet, side, data.to_vec()));
        Ok(())
    }

    fn finish(&mut self, sheet: u32, side: Side, height: u16) -> Result<(), Full> {
        self.seen.push(Page::Finish(sheet, side, height));
        Ok(())
    }
}

fn greeting() -> Vec<u8> {
    [0x10_u32.to_be_bytes(), *b"VENS", [0; 4], [0; 4]].concat()
}

fn frame(scsi_status: u32, data: &[u8]) -> Vec<u8> {
    let total = u32::try_from(data.len()).map_or(u32::MAX, |length| length.saturating_add(40));
    let mut reply = [
        total.to_be_bytes(),
        *b"VENS",
        [0; 4],
        scsi_status.to_be_bytes(),
    ]
    .concat();
    reply.resize(40, 0);
    reply.extend_from_slice(data);
    reply
}

fn sense([flags_and_key, code, qualifier]: [u8; 3]) -> Vec<u8> {
    let mut data = vec![0_u8; 18];
    for (offset, value) in [(2, flags_and_key), (12, code), (13, qualifier)] {
        if let Some(slot) = data.get_mut(offset) {
            *slot = value;
        }
    }
    data
}

fn pixel_size(height: u32) -> Vec<u8> {
    let mut data = vec![0_u8; 32];
    data.iter_mut()
        .skip(12)
        .zip(height.to_be_bytes())
        .for_each(|(slot, byte)| *slot = byte);
    data
}

fn setup() -> Vec<Vec<u8>> {
    [
        greeting(),
        frame(GOOD, &[]),
        frame(GOOD, &[]),
        frame(GOOD, &[]),
        frame(GOOD, &[0; 0x30]),
        frame(GOOD, &[0; 8]),
    ]
    .to_vec()
}

type Outcome = Result<Summary, ScanError<Closed, Full>>;

fn scan(replies: &[Vec<u8>], capacity: Option<usize>) -> Option<(Outcome, Vec<Pause>, Vec<Page>)> {
    let wire = Wire(replies.iter().flatten().copied().collect());
    let mut channel = CommandChannel::open(wire, HOST_MAC, HOST).ok()?;
    let mut clock = Pauses::default();
    let mut pages = Pages {
        seen: Vec::new(),
        capacity,
    };
    let mut buffer = [0_u8; 64];
    let outcome = run(&mut channel, &mut clock, &mut pages, &mut buffer);
    Some((outcome, clock.0, pages.seen))
}

fn one_sheet() -> Vec<Vec<u8>> {
    let mut replies = setup();
    replies.extend([
        frame(GOOD, &[]),
        frame(CHECK, &[]),
        frame(GOOD, &sense(NO_DATA_YET)),
        frame(CHECK, &[0, 0, 0xFF, 0xD8, 0xAA]),
        frame(GOOD, &sense(END_OF_SIDE)),
        frame(GOOD, &pixel_size(9856)),
        frame(CHECK, &[1, 0, 0xFF, 0xD8]),
        frame(GOOD, &sense(END_OF_SIDE)),
        frame(GOOD, &pixel_size(9000)),
        frame(CHECK, &[]),
        frame(GOOD, &sense(NO_PAPER)),
        frame(GOOD, &[]),
    ]);
    replies
}

#[test]
fn one_sheet_reaches_the_sink_as_two_finished_sides() {
    assert_eq!(
        scan(&one_sheet(), None),
        Some((
            Ok(Summary { sheets: 1 }),
            [Pause::NoDataYet].to_vec(),
            [
                Page::Append(0, Side::Front, [0xFF, 0xD8, 0xAA].to_vec()),
                Page::Finish(0, Side::Front, 9856),
                Page::Append(0, Side::Back, [0xFF, 0xD8].to_vec()),
                Page::Finish(0, Side::Back, 9000),
            ]
            .to_vec()
        ))
    );
}

#[test]
fn a_job_failure_is_reported_after_the_job_ends() {
    let mut replies = setup();
    replies.extend([
        frame(GOOD, &[]),
        frame(CHECK, &[]),
        frame(GOOD, &sense(JAM)),
        frame(GOOD, &[]),
    ]);
    assert_eq!(
        scan(&replies, None).map(|(outcome, _, _)| outcome),
        Some(Err(ScanError::Job(JobError::Condition(
            Condition::PaperJam
        ))))
    );
}

#[test]
fn a_sink_failure_stops_the_scan_with_its_cause() {
    assert_eq!(
        scan(&one_sheet(), Some(1)).map(|(outcome, _, pages)| (outcome, pages.len())),
        Some((Err(ScanError::Sink(Full)), 2))
    );
}

#[test]
fn a_stream_failure_stops_the_scan_with_its_cause() {
    assert_eq!(
        scan(&setup(), None).map(|(outcome, _, _)| outcome),
        Some(Err(ScanError::Channel(ChannelError::Port(Closed))))
    );
}
