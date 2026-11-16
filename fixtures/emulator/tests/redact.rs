//! Redaction turns a raw hardware trace into a fixture.
//! The fixture keeps every status, sense, and length, but carries documentation identifiers and synthetic image bytes.
//! The identifier fields follow sections 2.1, 5.1 to 5.4, and 7.1 of the ix1300-wifi protocol document.

use vens_core::{
    HostId, Ipv4Address, MacAddress, Port,
    control::{LocalTime, Release, Reserve, ReserveMode, UtcOffset},
    discovery::{Probe, SearchReply},
    jpeg::{FrameSize, frame_size},
    scan::{Command, Sequence, SheetIndex, Side},
};
use vens_emulator::redact::{
    DEVICE_NAME, HOST_ADDRESS, HOST_ID, OTHER_HOST_ADDRESS, RedactError, SCANNER_ADDRESS,
    SCANNER_MAC, SERIAL, TRIGGER_PORT, redact,
};
use vens_net::trace::{Channel, Direction, Event};

const PRIVATE_HOST: [u8; 4] = [10, 1, 2, 3];
const PRIVATE_SCANNER: [u8; 4] = [10, 1, 2, 9];
const PRIVATE_SCANNER_MAC: [u8; 6] = [0x84, 0x11, 0x22, 0x33, 0x44, 0x55];
const PRIVATE_HOST_ID: [u8; 8] = [0xC3, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
const PRIVATE_SERIAL: &[u8] = b"ZZZZ123456";
const PRIVATE_NAME: &[u8] = b"iX1300-ZZZZ123456";
const UNREGISTERED: [u8; 8] = [0xFF; 8];
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);
const GOOD: u32 = 0;
const CHECK: u32 = 2;
const PREFIX: usize = 2;
const STATUS_REPLY: usize = 6;
const RESERVE_REPLY: usize = 8;
const FIRST_IMAGE_HEADER: usize = 14;
const EVENTS_PER_CHUNK: usize = 3;
const SENSE_AFTER_ONE_CHUNK: usize = 18;
const SIZE: FrameSize = FrameSize {
    width: 2592,
    height: 3648,
};

fn event(channel: Channel, direction: Direction, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction,
        bytes: bytes.to_vec(),
    }
}

fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

fn search_reply() -> Vec<u8> {
    let mut reply = vec![0_u8; 144];
    put(&mut reply, 0, b"VENS");
    put(&mut reply, 16, &PRIVATE_SCANNER);
    put(&mut reply, 20, &53218_u32.to_be_bytes());
    put(&mut reply, 24, &53219_u32.to_be_bytes());
    put(&mut reply, 28, &PRIVATE_SCANNER_MAC);
    put(&mut reply, 40, PRIVATE_NAME);
    put(&mut reply, 104, b"ScanSnap iX1300 ");
    put(&mut reply, 120, &PRIVATE_HOST);
    put(&mut reply, 124, &PRIVATE_HOST_ID);
    put(&mut reply, 132, PRIVATE_SERIAL);
    reply
}

fn greeting() -> Vec<u8> {
    [0x10_u32.to_be_bytes(), *b"VENS", [0; 4], [0; 4]].concat()
}

fn reply_header(status: u32, payload: usize) -> Vec<u8> {
    let total = u32::try_from(payload).map_or(u32::MAX, |length| length.saturating_add(40));
    let mut header = [total.to_be_bytes(), *b"VENS", [0; 4], status.to_be_bytes()].concat();
    header.resize(40, 0);
    header
}

fn request(command: &Command, host: [u8; 4]) -> Vec<u8> {
    command
        .request(HOST_MAC, Ipv4Address::new(host))
        .encode()
        .to_vec()
}

fn status(host_id: [u8; 8]) -> Vec<u8> {
    let mut data = vec![0_u8; 0x30];
    put(&mut data, 0x20, &host_id);
    data
}

fn reserve(host: [u8; 4], port: u16, time: LocalTime, host_id: [u8; 8]) -> Vec<u8> {
    Reserve {
        mode: ReserveMode::Scan,
        mac: HOST_MAC,
        host: Ipv4Address::new(host),
        trigger_port: Port::new(port),
        time,
        host_id: HostId::new(host_id),
        offset: UtcOffset::from_seconds(0),
    }
    .encode()
    .to_vec()
}

fn control_reply(length: usize) -> Vec<u8> {
    let mut reply = [[0; 4], *b"VENS", [0; 4]].concat();
    reply.resize(length, 0);
    reply
}

fn sense(extra: &[u8]) -> Vec<u8> {
    let mut data = vec![0_u8; 18];
    put(&mut data, 2, &[0x40]);
    put(&mut data, 3, extra);
    data
}

fn image(comment: &[u8]) -> Vec<u8> {
    let comment_length =
        u16::try_from(comment.len()).map_or(u16::MAX, |length| length.saturating_add(2));
    [
        [0xFF, 0xD8, 0xFF, 0xFE].as_slice(),
        &comment_length.to_be_bytes(),
        comment,
        &[0xFF, 0xC0, 0x00, 0x11, 0x08],
        &SIZE.height.to_be_bytes(),
        &SIZE.width.to_be_bytes(),
        &[0x03, 0x01, 0x21, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01],
        &[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
        ],
        &[0x12, 0x34, 0x56, 0x78, 0x9A],
        &[0xFF, 0xD9],
    ]
    .concat()
}

struct Raw<'scan> {
    registered: [u8; 8],
    chunks: &'scan [Vec<u8>],
    sense_extra: &'scan [u8],
}

fn reading(host: [u8; 4], count: usize) -> Option<Vec<Vec<u8>>> {
    let mut requests = Vec::new();
    let mut sequence = Some(Sequence::FIRST);
    for _ in 0..count {
        let current = sequence?;
        requests.push(request(
            &Command::read_image(Side::Front, SheetIndex::FIRST, current),
            host,
        ));
        sequence = current.next();
    }
    Some(requests)
}

fn trace(scan: &Raw<'_>) -> Option<Vec<Event>> {
    use Channel::{Command as Cmd, Control, Discovery};
    use Direction::{Received, Sent};
    let start = Command::start_job();
    let mut events = [
        event(
            Discovery,
            Sent,
            &Probe::search(Ipv4Address::new(PRIVATE_HOST), HOST_MAC).encode(),
        ),
        event(Discovery, Received, &search_reply()),
        event(Cmd, Received, &greeting()),
        event(Control, Received, &greeting()),
        event(
            Cmd,
            Sent,
            &request(&Command::hardware_status(), PRIVATE_HOST),
        ),
        event(Cmd, Received, &reply_header(GOOD, 0x30)),
        event(Cmd, Received, &status(scan.registered)),
        event(
            Control,
            Sent,
            &reserve(
                PRIVATE_HOST,
                61000,
                LocalTime::new(2026, 11, 16, 3, 4, 5)?,
                PRIVATE_HOST_ID,
            ),
        ),
        event(Control, Received, &control_reply(20)),
        event(Cmd, Sent, &request(&start, PRIVATE_HOST)),
        event(Cmd, Sent, start.payload()),
        event(Cmd, Received, &reply_header(GOOD, 8)),
        event(Cmd, Received, &[0; 8]),
    ]
    .to_vec();
    let reads = reading(PRIVATE_HOST, scan.chunks.len())?;
    let last = scan.chunks.len().checked_sub(1)?;
    for (index, (chunk, read)) in scan.chunks.iter().zip(reads).enumerate() {
        let payload = [[0x00, u8::try_from(index).ok()?].as_slice(), chunk].concat();
        let status = if index == last { CHECK } else { GOOD };
        events.push(event(Cmd, Sent, &read));
        events.push(event(Cmd, Received, &reply_header(status, payload.len())));
        events.push(event(Cmd, Received, &payload));
    }
    events.extend([
        event(Cmd, Sent, &request(&Command::request_sense(), PRIVATE_HOST)),
        event(Cmd, Received, &reply_header(GOOD, 18)),
        event(Cmd, Received, &sense(scan.sense_extra)),
        event(Control, Sent, &Release::new(HOST_MAC).encode()),
        event(Control, Received, &control_reply(16)),
    ]);
    Some(events)
}

fn one_image() -> Vec<Vec<u8>> {
    [image(PRIVATE_SERIAL)].to_vec()
}

fn plain(chunks: &[Vec<u8>]) -> Raw<'_> {
    Raw {
        registered: PRIVATE_HOST_ID,
        chunks,
        sense_extra: &[],
    }
}

fn redacted(scan: &Raw<'_>) -> Option<Vec<Event>> {
    trace(scan).and_then(|events| redact(&events).ok())
}

fn bytes_of(events: &[Event], channel: Channel, direction: Direction) -> Vec<Vec<u8>> {
    events
        .iter()
        .filter(|event| event.channel == channel && event.direction == direction)
        .map(|event| event.bytes.clone())
        .collect()
}

fn image_reply(events: &[Event], chunk: usize) -> Option<(&[u8], &[u8])> {
    let header = FIRST_IMAGE_HEADER.checked_add(chunk.checked_mul(EVENTS_PER_CHUNK)?)?;
    Some((
        events.get(header)?.bytes.as_slice(),
        events.get(header.checked_add(1)?)?.bytes.as_slice(),
    ))
}

fn declared_payload(header: &[u8]) -> Option<usize> {
    let total = u32::from_be_bytes(*header.first_chunk::<4>()?);
    usize::try_from(total.checked_sub(40)?).ok()
}

#[test]
fn the_search_reply_carries_documentation_identifiers() {
    let chunks = one_image();
    let reply = redacted(&plain(&chunks)).and_then(|events| {
        bytes_of(&events, Channel::Discovery, Direction::Received)
            .first()
            .and_then(|bytes| SearchReply::decode(bytes).ok())
    });
    assert_eq!(
        reply.map(|reply| (
            reply.address().octets(),
            reply.mac().octets(),
            reply.device_name().as_str().map(str::to_owned),
            reply.serial().as_str().map(str::to_owned),
            reply.last_owner().bytes(),
        )),
        Some((
            SCANNER_ADDRESS,
            SCANNER_MAC,
            Some(DEVICE_NAME.to_owned()),
            Some(SERIAL.to_owned()),
            HOST_ID,
        ))
    );
}

#[test]
fn the_last_owner_address_becomes_a_documentation_address() {
    let chunks = one_image();
    let address = redacted(&plain(&chunks)).and_then(|events| {
        bytes_of(&events, Channel::Discovery, Direction::Received)
            .first()
            .and_then(|bytes| bytes.get(120..124).map(<[u8]>::to_vec))
    });
    assert_eq!(address, Some(OTHER_HOST_ADDRESS.to_vec()));
}

#[test]
fn every_frame_from_the_host_carries_the_documentation_address() {
    let chunks = one_image();
    let start = Command::start_job();
    let expected = reading(HOST_ADDRESS, 1).map(|reads| {
        [
            [
                Probe::search(Ipv4Address::new(HOST_ADDRESS), HOST_MAC)
                    .encode()
                    .to_vec(),
                request(&Command::hardware_status(), HOST_ADDRESS),
                request(&start, HOST_ADDRESS),
                start.payload().to_vec(),
            ]
            .to_vec(),
            reads,
            [request(&Command::request_sense(), HOST_ADDRESS)].to_vec(),
        ]
        .concat()
    });
    let sent = redacted(&plain(&chunks)).map(|events| {
        [
            bytes_of(&events, Channel::Discovery, Direction::Sent),
            bytes_of(&events, Channel::Command, Direction::Sent),
        ]
        .concat()
    });
    assert_eq!(sent, expected);
}

#[test]
fn the_reserve_carries_documentation_values_and_the_release_stays() {
    let chunks = one_image();
    let expected = LocalTime::new(2026, 1, 1, 0, 0, 0).map(|time| {
        [
            reserve(HOST_ADDRESS, TRIGGER_PORT, time, HOST_ID),
            Release::new(HOST_MAC).encode().to_vec(),
        ]
        .to_vec()
    });
    let sent = redacted(&plain(&chunks))
        .map(|events| bytes_of(&events, Channel::Control, Direction::Sent));
    assert_eq!(sent, expected);
}

#[test]
fn a_registered_identifier_in_the_status_reply_becomes_the_documentation_identifier() {
    let chunks = one_image();
    for (registered, expected) in [(PRIVATE_HOST_ID, HOST_ID), (UNREGISTERED, UNREGISTERED)] {
        let reply = redacted(&Raw {
            registered,
            ..plain(&chunks)
        })
        .and_then(|events| events.get(STATUS_REPLY).map(|event| event.bytes.clone()));
        assert_eq!(reply, Some(status(expected)));
    }
}

#[test]
fn image_bytes_become_a_synthetic_image_with_the_original_frame_size() {
    let chunks = one_image();
    let events = redacted(&plain(&chunks));
    let reply = events.as_deref().and_then(|events| image_reply(events, 0));
    assert_eq!(
        reply.map(|(header, payload)| (
            declared_payload(header) == Some(payload.len()),
            payload.get(..PREFIX),
            frame_size(payload.get(PREFIX..).unwrap_or_default()),
            payload.ends_with(&[0xFF, 0xD9]),
            payload
                .windows(PRIVATE_SERIAL.len())
                .any(|window| window == PRIVATE_SERIAL),
        )),
        Some((true, Some([0x00, 0x00].as_slice()), Ok(SIZE), true, false))
    );
}

#[test]
fn every_chunk_of_a_side_stays_non_empty_and_the_chunks_join_into_one_image() {
    let whole = image(PRIVATE_SERIAL);
    let chunks: Vec<Vec<u8>> = whole
        .chunks(whole.len().div_ceil(3))
        .map(<[u8]>::to_vec)
        .collect();
    let events = redacted(&plain(&chunks));
    let replies: Option<Vec<(&[u8], &[u8])>> = events.as_deref().and_then(|events| {
        (0..chunks.len())
            .map(|chunk| image_reply(events, chunk))
            .collect()
    });
    let shapes = replies.as_ref().map(|replies| {
        replies
            .iter()
            .map(|(header, payload)| {
                (
                    declared_payload(header) == Some(payload.len()),
                    payload.len() > PREFIX,
                    payload.get(1).copied(),
                )
            })
            .collect::<Vec<_>>()
    });
    let joined: Option<Vec<u8>> = replies.as_ref().map(|replies| {
        replies
            .iter()
            .flat_map(|(_, payload)| payload.get(PREFIX..).unwrap_or_default().iter().copied())
            .collect()
    });
    assert_eq!(
        shapes,
        Some(
            [
                (true, true, Some(0)),
                (true, true, Some(1)),
                (true, true, Some(2))
            ]
            .to_vec()
        )
    );
    assert_eq!(joined.as_deref().map(frame_size), Some(Ok(SIZE)));
}

#[test]
fn an_event_of_unknown_shape_is_refused_with_its_index() {
    let chunks = one_image();
    let refused = trace(&plain(&chunks)).map(|mut events| {
        events.insert(
            RESERVE_REPLY,
            event(Channel::Control, Direction::Received, &[0; 7]),
        );
        redact(&events)
    });
    assert_eq!(
        refused,
        Some(Err(RedactError::UnexpectedEvent {
            index: RESERVE_REPLY
        }))
    );
}

#[test]
fn a_reply_that_differs_from_its_declared_length_is_refused() {
    let chunks = one_image();
    let refused = trace(&plain(&chunks)).map(|mut events| {
        if let Some(reply) = events.get_mut(STATUS_REPLY) {
            reply.bytes.pop();
        }
        redact(&events)
    });
    assert_eq!(
        refused,
        Some(Err(RedactError::UnexpectedEvent {
            index: STATUS_REPLY
        }))
    );
}

#[test]
fn an_identifier_left_in_another_field_is_refused() {
    let chunks = one_image();
    let refused = trace(&Raw {
        sense_extra: PRIVATE_SERIAL,
        ..plain(&chunks)
    })
    .map(|events| redact(&events));
    assert_eq!(
        refused,
        Some(Err(RedactError::IdentifierSurvived {
            index: SENSE_AFTER_ONE_CHUNK
        }))
    );
}

#[test]
fn a_side_without_a_frame_header_is_refused() {
    let chunks = [[0xFF, 0xD8, 0x12, 0x34, 0xFF, 0xD9].to_vec()];
    let refused = trace(&plain(&chunks)).map(|events| redact(&events));
    assert_eq!(
        refused,
        Some(Err(RedactError::MissingFrameHeader {
            index: FIRST_IMAGE_HEADER + 1
        }))
    );
}
