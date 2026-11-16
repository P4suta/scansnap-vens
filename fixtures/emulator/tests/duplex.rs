//! The fixture holds one real duplex scan of one sheet, recorded on the scanner and then redacted.
//! The session must replay it byte for byte, in the recorded order across both connections.
//! It must write both sides at the height that the scanner reported.

use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::mpsc,
};

use vens_core::{
    Ipv4Address, MacAddress, Port,
    control::LocalTime,
    discovery::{Probe, SearchReply},
    job::{Pause, Summary},
    jpeg::{self, FrameSize, JpegError, frame_size},
    scan::Side,
};
use vens_emulator::{
    redact::{
        DEVICE_NAME, HOST_ADDRESS, HOST_ID, SCANNER_ADDRESS, SCANNER_MAC, SERIAL, TRIGGER_PORT,
    },
    replay::{DatagramReplay, Replay, ReplayError},
};
use vens_net::{
    channel::{ChannelError, CommandChannel},
    control::ControlChannel,
    record::Recorder,
    scan::{Clock, PageSink},
    search::search,
    session::{Host, SessionError, scan_once},
    trace::{self, Channel, Direction, Event},
};

const FIXTURE: &str = include_str!("../traces/one-duplex-sheet.trace");
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);
const RECORDED_SIZE: FrameSize = FrameSize {
    width: 2592,
    height: 3648,
};
const BUSY_REPLIES: usize = 14;

#[derive(Default)]
struct Pauses(Vec<Pause>);

impl Clock for Pauses {
    fn wait(&mut self, pause: Pause) {
        self.0.push(pause);
    }
}

#[derive(Default)]
struct Images {
    current: Vec<u8>,
    finished: Vec<(u32, Side, Vec<u8>)>,
}

impl PageSink for Images {
    type Error = JpegError;

    fn append(&mut self, _: u32, _: Side, data: &[u8]) -> Result<(), JpegError> {
        self.current.extend_from_slice(data);
        Ok(())
    }

    fn finish(&mut self, sheet: u32, side: Side, height: u16) -> Result<(), JpegError> {
        let mut image = std::mem::take(&mut self.current);
        jpeg::set_height(&mut image, height)?;
        self.finished.push((sheet, side, image));
        Ok(())
    }
}

type Outcome = Result<Summary, SessionError<ReplayError, JpegError>>;

struct Replayed {
    outcome: Outcome,
    pauses: Vec<Pause>,
    images: Vec<(u32, Side, Vec<u8>)>,
    finished: (bool, bool),
    order: Vec<Event>,
}

fn events() -> Vec<Event> {
    trace::parse(FIXTURE).unwrap_or_default()
}

fn replay(host_address: [u8; 4]) -> Option<Replayed> {
    let events = events();
    let (log, order) = mpsc::channel();
    let command_port = Recorder::new(
        Replay::new(&events, Channel::Command),
        Channel::Command,
        log.clone(),
    );
    let control_port = Recorder::new(
        Replay::new(&events, Channel::Control),
        Channel::Control,
        log,
    );
    let mut command =
        CommandChannel::open(command_port, HOST_MAC, Ipv4Address::new(host_address)).ok()?;
    let mut control = ControlChannel::open(control_port).ok()?;
    let host = Host {
        mac: HOST_MAC,
        address: Ipv4Address::new(host_address),
        trigger_port: Port::new(TRIGGER_PORT),
        time: LocalTime::new(2026, 1, 1, 0, 0, 0)?,
    };
    let mut pauses = Pauses::default();
    let mut images = Images::default();
    let mut buffer = vec![0_u8; 0x0030_0040];
    let outcome = scan_once(
        &mut command,
        &mut control,
        host,
        &mut pauses,
        &mut images,
        &mut buffer,
    );
    Some(Replayed {
        outcome,
        pauses: pauses.0,
        images: images.finished,
        finished: (
            command.into_stream().into_port().finished(),
            control.into_stream().into_port().finished(),
        ),
        order: order.try_iter().collect(),
    })
}

#[test]
fn the_session_keeps_the_recorded_order_across_both_connections() {
    let recorded: Vec<Event> = events()
        .into_iter()
        .filter(|event| event.channel != Channel::Discovery)
        .collect();
    assert_eq!(
        replay(HOST_ADDRESS).map(|replayed| replayed.order),
        Some(recorded)
    );
}

#[test]
fn the_fixture_parses() {
    assert_eq!(trace::parse(FIXTURE).map(|events| events.len()), Ok(101));
}

fn first_with_length(channel: Channel, direction: Direction, length: usize) -> Option<Vec<u8>> {
    events()
        .into_iter()
        .find(|event| {
            event.channel == channel && event.direction == direction && event.bytes.len() == length
        })
        .map(|event| event.bytes)
}

#[test]
fn the_fixture_carries_documentation_identifiers_only() {
    let reply = first_with_length(Channel::Discovery, Direction::Received, 144)
        .and_then(|bytes| SearchReply::decode(&bytes).ok());
    let status = first_with_length(Channel::Command, Direction::Received, 0x30);
    let reserve = first_with_length(Channel::Control, Direction::Sent, 128);
    assert_eq!(
        (
            reply.map(|reply| (
                reply.mac().octets(),
                reply.device_name().as_str().map(str::to_owned),
                reply.last_owner().bytes(),
            )),
            status.and_then(|bytes| bytes.get(0x20..0x28).map(<[u8]>::to_vec)),
            reserve.map(|bytes| (
                bytes.get(44..48).map(<[u8]>::to_vec),
                bytes.get(108..116).map(<[u8]>::to_vec),
            )),
        ),
        (
            Some((SCANNER_MAC, Some(DEVICE_NAME.to_owned()), HOST_ID)),
            Some(HOST_ID.to_vec()),
            Some((Some(HOST_ADDRESS.to_vec()), Some(HOST_ID.to_vec()))),
        )
    );
}

#[test]
fn discovery_replays_with_documentation_identifiers() {
    let events = events();
    let mut discovery = DatagramReplay::new(&events);
    let target = SocketAddrV4::new(Ipv4Addr::from(SCANNER_ADDRESS), 52217);
    let reply = search(
        &mut discovery,
        Probe::search(Ipv4Address::new(HOST_ADDRESS), HOST_MAC),
        target,
    );
    assert_eq!(
        reply.map(|reply| (
            reply.address().octets(),
            reply.command_port().number(),
            reply.control_port().number(),
            reply.serial().as_str().map(str::to_owned),
        )),
        Ok((SCANNER_ADDRESS, 53218, 53219, Some(SERIAL.to_owned())))
    );
    assert!(discovery.finished());
}

#[test]
fn the_session_replays_the_recorded_scan_and_writes_both_sides_at_the_recorded_height() {
    let replayed = replay(HOST_ADDRESS);
    let expected_pauses = [[Pause::Busy; BUSY_REPLIES].as_slice(), &[Pause::NoDataYet]].concat();
    assert_eq!(
        replayed.map(|replayed| (
            replayed.outcome,
            replayed.pauses,
            replayed
                .images
                .iter()
                .map(|(sheet, side, image)| (*sheet, *side, frame_size(image)))
                .collect::<Vec<_>>(),
            replayed.finished,
        )),
        Some((
            Ok(Summary { sheets: 1 }),
            expected_pauses,
            [
                (0, Side::Front, Ok(RECORDED_SIZE)),
                (0, Side::Back, Ok(RECORDED_SIZE))
            ]
            .to_vec(),
            (true, true),
        ))
    );
}

#[test]
fn a_request_that_differs_by_one_byte_is_refused() {
    assert_eq!(
        replay([192, 0, 2, 13]).map(|replayed| replayed.outcome),
        Some(Err(SessionError::Command(ChannelError::Port(
            ReplayError::Diverged {
                offset: 27,
                expected: 12,
                actual: 13
            }
        ))))
    );
}
