//! Drives the pure job over a scripted stream with the replies of section 8.1 of the ix1300-wifi protocol document.
//! The keepalive cadence during a long job follows sections 5.1 and 5.3.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddrV4},
};

use vens_core::{
    Ipv4Address, MacAddress,
    discovery::Probe,
    job::{JobError, Pause, Summary},
    keepalive::Elapsed,
    scan::Side,
    sense::Condition,
};
use vens_net::{
    channel::{ChannelError, CommandChannel, Stream},
    keepalive::{DatagramKeepalive, Keepalive},
    scan::{Clock, PageSink, ScanError, run},
    search::Datagram,
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

/// Advances a fixed step on every pause, so the elapsed time is part of the script.
struct Virtual {
    pauses: Vec<Pause>,
    now: u64,
    step: u64,
}

impl Virtual {
    const fn stepping(step: u64) -> Self {
        Self {
            pauses: Vec::new(),
            now: 0,
            step,
        }
    }
}

impl Clock for Virtual {
    fn wait(&mut self, pause: Pause) {
        self.pauses.push(pause);
        self.now = self.now.saturating_add(self.step);
    }

    fn now(&mut self) -> Elapsed {
        Elapsed::from_millis(self.now)
    }
}

#[derive(Default)]
struct Ticks(Vec<u64>);

impl Keepalive for Ticks {
    fn tick(&mut self, now: Elapsed) {
        self.0.push(now.millis());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Down;

/// Answers every keepalive probe at once, as a scanner does for the host that holds the reserve.
#[derive(Default)]
struct Air {
    inbox: VecDeque<Vec<u8>>,
    sent: usize,
}

impl Datagram for Air {
    type Error = Down;

    fn send(&mut self, _: &[u8], _: SocketAddrV4) -> Result<(), Down> {
        let mut answer = b"VENS".to_vec();
        answer.extend_from_slice(&[0x80, 0x01]);
        answer.resize(12, 0);
        self.inbox.push_back(answer);
        self.sent = self.sent.saturating_add(1);
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

fn drive(
    replies: &[Vec<u8>],
    capacity: Option<usize>,
    clock: &mut Virtual,
    keepalive: &mut impl Keepalive,
) -> Option<(Outcome, Vec<Page>)> {
    let wire = Wire(replies.iter().flatten().copied().collect());
    let mut channel = CommandChannel::open(wire, HOST_MAC, HOST).ok()?;
    let mut pages = Pages {
        seen: Vec::new(),
        capacity,
    };
    let mut buffer = [0_u8; 64];
    let outcome = run(&mut channel, clock, keepalive, &mut pages, &mut buffer);
    Some((outcome, pages.seen))
}

fn scan(replies: &[Vec<u8>], capacity: Option<usize>) -> Option<(Outcome, Vec<Pause>, Vec<Page>)> {
    let mut clock = Virtual::stepping(0);
    let (outcome, pages) = drive(replies, capacity, &mut clock, &mut Ticks::default())?;
    Some((outcome, clock.pauses, pages))
}

fn sheet(front: &[u8], back: &[u8]) -> Vec<Vec<u8>> {
    [
        frame(GOOD, &[]),
        frame(CHECK, &[]),
        frame(GOOD, &sense(NO_DATA_YET)),
        frame(CHECK, front),
        frame(GOOD, &sense(END_OF_SIDE)),
        frame(GOOD, &pixel_size(9856)),
        frame(CHECK, back),
        frame(GOOD, &sense(END_OF_SIDE)),
        frame(GOOD, &pixel_size(9000)),
    ]
    .to_vec()
}

fn end() -> Vec<Vec<u8>> {
    [
        frame(CHECK, &[]),
        frame(GOOD, &sense(NO_PAPER)),
        frame(GOOD, &[]),
    ]
    .to_vec()
}

fn one_sheet() -> Vec<Vec<u8>> {
    [
        setup(),
        sheet(&[0, 0, 0xFF, 0xD8, 0xAA], &[1, 0, 0xFF, 0xD8]),
        end(),
    ]
    .concat()
}

fn stack(sheets: u8) -> Vec<Vec<u8>> {
    let mut replies = setup();
    for index in 0..sheets {
        let front = index.saturating_mul(2);
        replies.extend(sheet(
            &[front, 0, 0xFF, 0xD8],
            &[front.saturating_add(1), 0, 0xFF, 0xD8],
        ));
    }
    replies.extend(end());
    replies
}

#[test]
fn the_keepalive_ticks_before_every_command_with_the_elapsed_time() {
    let mut clock = Virtual::stepping(1000);
    let mut ticks = Ticks::default();
    let outcome = drive(&one_sheet(), None, &mut clock, &mut ticks).map(|(outcome, _)| outcome);
    let expected: Vec<u64> = [[0; 8].as_slice(), &[1000; 9]].concat();
    assert_eq!(
        (outcome, ticks.0),
        (Some(Ok(Summary { sheets: 1 })), expected)
    );
}

#[test]
fn a_long_stack_sends_one_keepalive_in_each_round_to_a_scanner_that_answers() {
    let mut clock = Virtual::stepping(5000);
    let mut keepalive = DatagramKeepalive::new(
        Air::default(),
        Probe::keepalive(HOST, HOST_MAC),
        SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 7), 52217),
        Elapsed::from_millis(0),
    );
    let outcome = drive(&stack(8), None, &mut clock, &mut keepalive).map(|(outcome, _)| outcome);
    let replies = keepalive.replies();
    let (air, failure) = keepalive.into_parts();
    assert_eq!(
        (outcome, clock.now, air.sent, replies, failure),
        (Some(Ok(Summary { sheets: 8 })), 40_000, 3, 3, None)
    );
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
