//! One scan session: read the registered host identifier, reserve with it, scan, and release.
//! The order follows sections 2.1, 5.3, 5.4, and 7.3 of the ix1300-wifi protocol document.

use std::collections::VecDeque;

use vens_core::{
    HostId, Ipv4Address, MacAddress, Port,
    control::{LocalTime, Release, Reserve, ReserveMode, ReserveRefusal, UtcOffset},
    job::{JobError, Pause, Summary},
    scan::Side,
    sense::Condition,
};
use vens_net::{
    channel::{CommandChannel, Stream},
    control::{ControlChannel, ControlError},
    scan::{Clock, PageSink, ScanError},
    session::{Host, SessionError, scan_once},
};

const ADDRESS: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);
const HOST_ID: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
const GOOD: u32 = 0;
const CHECK: u32 = 2;

#[derive(Debug, PartialEq, Eq)]
struct Closed;

#[derive(Debug, PartialEq, Eq)]
struct Never;

struct Wire {
    incoming: VecDeque<u8>,
    outgoing: Vec<u8>,
}

impl Wire {
    fn new(incoming: &[Vec<u8>]) -> Self {
        Self {
            incoming: incoming.iter().flatten().copied().collect(),
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

struct Instant;

impl Clock for Instant {
    fn wait(&mut self, _: Pause) {}
}

#[derive(Default)]
struct Finished(Vec<(u32, Side, u16)>);

impl PageSink for Finished {
    type Error = Never;

    fn append(&mut self, _: u32, _: Side, _: &[u8]) -> Result<(), Never> {
        Ok(())
    }

    fn finish(&mut self, sheet: u32, side: Side, height: u16) -> Result<(), Never> {
        self.0.push((sheet, side, height));
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

fn control_reply(length: usize, result: i32) -> Vec<u8> {
    let mut reply = [[0; 4], *b"VENS", result.to_be_bytes()].concat();
    reply.resize(length, 0);
    reply
}

fn status(host_id: [u8; 8]) -> Vec<u8> {
    let mut data = vec![0_u8; 0x30];
    data.iter_mut()
        .skip(0x20)
        .zip(host_id)
        .for_each(|(slot, byte)| *slot = byte);
    data
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

fn job(sheet: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut replies = [
        greeting(),
        frame(GOOD, &status(HOST_ID)),
        frame(GOOD, &[]),
        frame(GOOD, &[]),
        frame(GOOD, &[]),
        frame(GOOD, &status(HOST_ID)),
        frame(GOOD, &[0; 8]),
        frame(GOOD, &[]),
    ]
    .to_vec();
    replies.extend_from_slice(sheet);
    replies
}

fn one_sheet() -> Vec<Vec<u8>> {
    job(&[
        frame(CHECK, &[0, 0, 0xFF]),
        frame(GOOD, &sense([0x40, 0, 0])),
        frame(GOOD, &pixel_size(9856)),
        frame(CHECK, &[1, 0, 0xFF]),
        frame(GOOD, &sense([0x40, 0, 0])),
        frame(GOOD, &pixel_size(9000)),
        frame(CHECK, &[]),
        frame(GOOD, &sense([3, 0x80, 0x03])),
        frame(GOOD, &[]),
    ])
}

fn host() -> Option<Host> {
    Some(Host {
        mac: MAC,
        address: ADDRESS,
        trigger_port: Port::new(55265),
        time: LocalTime::from_unix_seconds(1_700_000_000)?,
    })
}

fn expected_reserve() -> Option<Vec<u8>> {
    host().map(|host| {
        Reserve {
            mode: ReserveMode::Scan,
            mac: host.mac,
            host: host.address,
            trigger_port: host.trigger_port,
            time: host.time,
            host_id: HostId::new(HOST_ID),
            offset: UtcOffset::from_seconds(0),
        }
        .encode()
        .to_vec()
    })
}

type Outcome = Result<Summary, SessionError<Closed, Never>>;

fn session(command: &[Vec<u8>], control: &[Vec<u8>]) -> Option<(Outcome, Vec<u8>, usize)> {
    let mut command = CommandChannel::open(Wire::new(command), MAC, ADDRESS).ok()?;
    let mut control = ControlChannel::open(Wire::new(control)).ok()?;
    let mut sink = Finished::default();
    let mut buffer = [0_u8; 64];
    let outcome = scan_once(
        &mut command,
        &mut control,
        host()?,
        &mut Instant,
        &mut sink,
        &mut buffer,
    );
    Some((outcome, control.into_stream().outgoing, sink.0.len()))
}

#[test]
fn a_session_reserves_with_the_registered_identifier_and_releases_after_the_job() {
    let control = [greeting(), control_reply(20, 0), control_reply(16, 0)];
    let sent = expected_reserve()
        .map(|reserve| [reserve.as_slice(), Release::new(MAC).encode().as_slice()].concat());
    assert_eq!(
        session(&one_sheet(), &control),
        sent.map(|sent| (Ok(Summary { sheets: 1 }), sent, 2))
    );
}

#[test]
fn a_status_reply_with_a_few_extra_bytes_still_yields_the_identifier() {
    let mut long_status = status([0xFF; 8]);
    long_status.extend_from_slice(&[0, 0]);
    let command = [greeting(), frame(GOOD, &long_status)];
    assert_eq!(
        session(&command, &[greeting()]).map(|(outcome, _, _)| outcome),
        Some(Err(SessionError::NotRegistered))
    );
}

#[test]
fn a_scanner_without_a_registered_identifier_is_never_reserved() {
    let command = [greeting(), frame(GOOD, &status([0xFF; 8]))];
    assert_eq!(
        session(&command, &[greeting()]),
        Some((Err(SessionError::NotRegistered), Vec::new(), 0))
    );
}

#[test]
fn a_refused_reserve_ends_the_session_before_any_scan_command() {
    let control = [greeting(), control_reply(20, -4)];
    assert_eq!(
        session(&one_sheet(), &control).map(|(outcome, _, sides)| (outcome, sides)),
        Some((
            Err(SessionError::Reserve(ControlError::Reserve(
                ReserveRefusal::Occupied
            ))),
            0
        ))
    );
}

fn jammed_job() -> Vec<Vec<u8>> {
    job(&[
        frame(CHECK, &[]),
        frame(GOOD, &sense([3, 0x80, 0x01])),
        frame(GOOD, &[]),
    ])
}

#[test]
fn a_failed_job_still_releases_the_scanner_and_reports_the_job_failure() {
    let control = [greeting(), control_reply(20, 0), control_reply(16, 0)];
    let sent = expected_reserve()
        .map(|reserve| [reserve.as_slice(), Release::new(MAC).encode().as_slice()].concat());
    assert_eq!(
        session(&jammed_job(), &control).map(|(outcome, sent, _)| (outcome, sent)),
        sent.map(|sent| (
            Err(SessionError::Scan {
                failure: ScanError::Job(JobError::Condition(Condition::PaperJam)),
                release: Ok(()),
            }),
            sent
        ))
    );
}

#[test]
fn a_failed_job_with_a_refused_release_reports_both_failures() {
    let control = [greeting(), control_reply(20, 0), control_reply(16, -1)];
    assert_eq!(
        session(&jammed_job(), &control).map(|(outcome, _, _)| outcome),
        Some(Err(SessionError::Scan {
            failure: ScanError::Job(JobError::Condition(Condition::PaperJam)),
            release: Err(ControlError::Release(-1)),
        }))
    );
}

#[test]
fn a_refused_release_after_a_good_job_keeps_the_job_summary() {
    let control = [greeting(), control_reply(20, 0), control_reply(16, -1)];
    assert_eq!(
        session(&one_sheet(), &control).map(|(outcome, _, sides)| (outcome, sides)),
        Some((
            Err(SessionError::Release {
                summary: Summary { sheets: 1 },
                failure: ControlError::Release(-1),
            }),
            2
        ))
    );
}
