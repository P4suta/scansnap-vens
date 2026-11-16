//! Redaction turns a raw hardware trace into a fixture.
//! The fixture keeps every status, sense key, and request length, but carries documentation identifiers and synthetic image bytes.
//! The identifier fields follow sections 2.1, 5.1 to 5.4, and 7.1 of the ix1300-wifi protocol document.
//! The read residue in the sense information field follows section 8.10.

use vens_core::{
    HostId, Ipv4Address, MacAddress, Port,
    control::{LocalTime, Release, Reserve, ReserveMode, UtcOffset},
    discovery::{Probe, SearchReply},
    jpeg::{FrameSize, frame_size},
    scan::{Command, Sequence, SheetIndex, Side},
};
use vens_emulator::redact::{
    DEVICE_NAME, HOST_ADDRESS, HOST_ID, HOST_MAC, OTHER_HOST_ADDRESS, RedactError, SCANNER_ADDRESS,
    SCANNER_MAC, SERIAL, TRIGGER_PORT, redact,
};
use vens_net::trace::{Channel, Direction, Event};

const PRIVATE_HOST: [u8; 4] = [10, 1, 2, 3];
const PRIVATE_HOST_MAC: [u8; 6] = [0x3C, 0x22, 0xFB, 0x01, 0x02, 0x03];
const PRIVATE_SCANNER: [u8; 4] = [10, 1, 2, 9];
const PRIVATE_SCANNER_MAC: [u8; 6] = [0x84, 0x11, 0x22, 0x33, 0x44, 0x55];
const PRIVATE_HOST_ID: [u8; 8] = [0xC3, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
const PRIVATE_SERIAL: &[u8] = b"ZZZZ123456";
const PADDED_SERIAL: &[u8] = b"ZZZZ123456  ";
const PRIVATE_NAME: &[u8] = b"iX1300-ZZZZ123456";
const UNREGISTERED: [u8; 8] = [0xFF; 8];
const NO_IDENTIFIER: [u8; 8] = [0; 8];
const TOKYO: i32 = 32_400;
const GOOD: u32 = 0;
const CHECK: u32 = 2;
const BUSY: u32 = 8;
const PREFIX: usize = 2;
const READ_LENGTH: u32 = 0x0030_0000;
const BUSY_PAYLOAD: usize = 34;
const INFORMATION: usize = 3;
const RESERVE_HOST_ID: usize = 108;
const FRONT: FrameSize = FrameSize {
    width: 2592,
    height: 3648,
};
const BACK: FrameSize = FrameSize {
    width: 2592,
    height: 1000,
};

fn event(channel: Channel, direction: Direction, bytes: &[u8]) -> Event {
    Event {
        channel,
        direction,
        bytes: bytes.to_vec(),
    }
}

fn push(events: &mut Vec<Event>, channel: Channel, direction: Direction, bytes: &[u8]) -> usize {
    let index = events.len();
    events.push(event(channel, direction, bytes));
    index
}

fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

fn search_reply(serial: &[u8]) -> Vec<u8> {
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
    put(&mut reply, 132, serial);
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

fn request(command: &Command, mac: [u8; 6], host: [u8; 4]) -> Vec<u8> {
    command
        .request(MacAddress::new(mac), Ipv4Address::new(host))
        .encode()
        .to_vec()
}

fn status(host_id: [u8; 8]) -> Vec<u8> {
    let mut data = vec![0_u8; 0x30];
    put(&mut data, 0x20, &host_id);
    data
}

struct Reservation {
    mac: [u8; 6],
    host: [u8; 4],
    port: u16,
    time: LocalTime,
    host_id: [u8; 8],
    offset: i32,
}

fn reserve(reservation: &Reservation) -> Vec<u8> {
    Reserve {
        mode: ReserveMode::Scan,
        mac: MacAddress::new(reservation.mac),
        host: Ipv4Address::new(reservation.host),
        trigger_port: Port::new(reservation.port),
        time: reservation.time,
        host_id: HostId::new(reservation.host_id),
        offset: UtcOffset::from_seconds(reservation.offset),
    }
    .encode()
    .to_vec()
}

fn control_reply(length: usize) -> Vec<u8> {
    let mut reply = [[0; 4], *b"VENS", [0; 4]].concat();
    reply.resize(length, 0);
    reply
}

fn end_of_side(information: u32) -> Vec<u8> {
    let mut data = vec![0_u8; 18];
    put(&mut data, 0, &[0xF0]);
    put(&mut data, 2, &[0x60]);
    put(&mut data, INFORMATION, &information.to_be_bytes());
    put(&mut data, 7, &[0x0A]);
    data
}

fn image(size: FrameSize) -> Vec<u8> {
    let comment_length =
        u16::try_from(PRIVATE_SERIAL.len()).map_or(u16::MAX, |length| length.saturating_add(2));
    [
        [0xFF, 0xD8, 0xFF, 0xFE].as_slice(),
        &comment_length.to_be_bytes(),
        PRIVATE_SERIAL,
        &[0xFF, 0xC0, 0x00, 0x11, 0x08],
        &size.height.to_be_bytes(),
        &size.width.to_be_bytes(),
        &[0x03, 0x01, 0x21, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01],
        &[
            0xFF, 0xDA, 0x00, 0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00,
        ],
        &[0x12, 0x34, 0x56, 0x78, 0x9A],
        &[0xFF, 0xD9],
    ]
    .concat()
}

struct Pass {
    side: Side,
    chunks: Vec<Vec<u8>>,
}

fn pass(side: Side, frame: FrameSize, pieces: usize) -> Pass {
    let whole = image(frame);
    Pass {
        side,
        chunks: whole
            .chunks(whole.len().div_ceil(pieces))
            .map(<[u8]>::to_vec)
            .collect(),
    }
}

struct Raw<'scan> {
    registered: [u8; 8],
    serial: &'scan [u8],
    busy: Option<&'scan [u8]>,
    passes: &'scan [Pass],
}

fn plain(passes: &[Pass]) -> Raw<'_> {
    Raw {
        registered: PRIVATE_HOST_ID,
        serial: PRIVATE_SERIAL,
        busy: None,
        passes,
    }
}

#[derive(Debug, Default)]
struct Layout {
    status_reply: usize,
    reserve: usize,
    busy: Option<usize>,
    chunks: Vec<Vec<(usize, usize)>>,
    senses: Vec<usize>,
}

fn trace(scan: &Raw<'_>) -> Option<(Vec<Event>, Layout)> {
    use Channel::{Command as Cmd, Control, Discovery};
    use Direction::{Received, Sent};
    let mac = PRIVATE_HOST_MAC;
    let start = Command::start_job();
    let mut layout = Layout::default();
    let mut events = Vec::new();
    let probe = Probe::search(Ipv4Address::new(PRIVATE_HOST), MacAddress::new(mac)).encode();
    push(&mut events, Discovery, Sent, &probe);
    push(&mut events, Discovery, Received, &search_reply(scan.serial));
    push(&mut events, Cmd, Received, &greeting());
    push(&mut events, Control, Received, &greeting());
    let hardware_status = request(&Command::hardware_status(), mac, PRIVATE_HOST);
    push(&mut events, Cmd, Sent, &hardware_status);
    push(&mut events, Cmd, Received, &reply_header(GOOD, 0x30));
    layout.status_reply = push(&mut events, Cmd, Received, &status(scan.registered));
    let reservation = Reservation {
        mac,
        host: PRIVATE_HOST,
        port: 61000,
        time: LocalTime::new(2026, 11, 16, 3, 4, 5)?,
        host_id: scan.registered,
        offset: TOKYO,
    };
    layout.reserve = push(&mut events, Control, Sent, &reserve(&reservation));
    push(&mut events, Control, Received, &control_reply(20));
    push(&mut events, Cmd, Sent, &request(&start, mac, PRIVATE_HOST));
    push(&mut events, Cmd, Sent, start.payload());
    push(&mut events, Cmd, Received, &reply_header(GOOD, 8));
    push(&mut events, Cmd, Received, &[0; 8]);
    if let Some(busy) = scan.busy {
        let read = Command::read_image(Side::Front, SheetIndex::FIRST, Sequence::FIRST);
        push(&mut events, Cmd, Sent, &request(&read, mac, PRIVATE_HOST));
        push(&mut events, Cmd, Received, &reply_header(BUSY, busy.len()));
        layout.busy = Some(push(&mut events, Cmd, Received, busy));
    }
    for scanned in scan.passes {
        let page = match scanned.side {
            Side::Front => 0,
            Side::Back => 1,
        };
        let last = scanned.chunks.len().checked_sub(1)?;
        let mut sequence = Some(Sequence::FIRST);
        let mut chunks = Vec::new();
        let mut final_length = 0;
        for (index, chunk) in scanned.chunks.iter().enumerate() {
            let current = sequence?;
            let payload = [[page, u8::try_from(index).ok()?].as_slice(), chunk].concat();
            let status = if index == last { CHECK } else { GOOD };
            let read = Command::read_image(scanned.side, SheetIndex::FIRST, current);
            push(&mut events, Cmd, Sent, &request(&read, mac, PRIVATE_HOST));
            let header = push(
                &mut events,
                Cmd,
                Received,
                &reply_header(status, payload.len()),
            );
            let body = push(&mut events, Cmd, Received, &payload);
            chunks.push((header, body));
            final_length = payload.len();
            sequence = current.next();
        }
        let information = READ_LENGTH.checked_sub(u32::try_from(final_length).ok()?)?;
        push(
            &mut events,
            Cmd,
            Sent,
            &request(&Command::request_sense(), mac, PRIVATE_HOST),
        );
        push(&mut events, Cmd, Received, &reply_header(GOOD, 18));
        layout
            .senses
            .push(push(&mut events, Cmd, Received, &end_of_side(information)));
        layout.chunks.push(chunks);
    }
    let release = Release::new(MacAddress::new(mac)).encode();
    push(&mut events, Control, Sent, &release);
    push(&mut events, Control, Received, &control_reply(16));
    Some((events, layout))
}

fn redacted(scan: &Raw<'_>) -> Option<(Vec<Event>, Layout)> {
    trace(scan).and_then(|(events, layout)| redact(&events).ok().map(|events| (events, layout)))
}

fn refused(
    scan: &Raw<'_>,
    change: impl FnOnce(&mut Vec<Event>, &Layout),
) -> Option<(Result<Vec<Event>, RedactError>, Layout)> {
    trace(scan).map(|(mut events, layout)| {
        change(&mut events, &layout);
        (redact(&events), layout)
    })
}

fn bytes_of(events: &[Event], channel: Channel, direction: Direction) -> Vec<Vec<u8>> {
    events
        .iter()
        .filter(|event| event.channel == channel && event.direction == direction)
        .map(|event| event.bytes.clone())
        .collect()
}

fn bytes_at(events: &[Event], index: usize) -> &[u8] {
    events
        .get(index)
        .map_or(&[], |event| event.bytes.as_slice())
}

fn declared_payload(header: &[u8]) -> Option<usize> {
    let total = u32::from_be_bytes(*header.first_chunk::<4>()?);
    usize::try_from(total.checked_sub(40)?).ok()
}

fn joined(events: &[Event], chunks: &[(usize, usize)]) -> Vec<u8> {
    chunks
        .iter()
        .flat_map(|(_, payload)| {
            bytes_at(events, *payload)
                .get(PREFIX..)
                .unwrap_or_default()
                .iter()
                .copied()
        })
        .collect()
}

fn one_front() -> Vec<Pass> {
    [pass(Side::Front, FRONT, 1)].into_iter().collect()
}

#[test]
fn the_search_reply_carries_documentation_identifiers() {
    let passes = one_front();
    let reply = redacted(&plain(&passes)).and_then(|(events, _)| {
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
    let passes = one_front();
    let address = redacted(&plain(&passes)).and_then(|(events, _)| {
        bytes_of(&events, Channel::Discovery, Direction::Received)
            .first()
            .and_then(|bytes| bytes.get(120..124).map(<[u8]>::to_vec))
    });
    assert_eq!(address, Some(OTHER_HOST_ADDRESS.to_vec()));
}

#[test]
fn every_frame_from_the_host_carries_the_documentation_address_and_mac() {
    let passes = one_front();
    let start = Command::start_job();
    let read = Command::read_image(Side::Front, SheetIndex::FIRST, Sequence::FIRST);
    let expected = [
        Probe::search(Ipv4Address::new(HOST_ADDRESS), MacAddress::new(HOST_MAC))
            .encode()
            .to_vec(),
        request(&Command::hardware_status(), HOST_MAC, HOST_ADDRESS),
        request(&start, HOST_MAC, HOST_ADDRESS),
        start.payload().to_vec(),
        request(&read, HOST_MAC, HOST_ADDRESS),
        request(&Command::request_sense(), HOST_MAC, HOST_ADDRESS),
    ]
    .to_vec();
    let sent = redacted(&plain(&passes)).map(|(events, _)| {
        [
            bytes_of(&events, Channel::Discovery, Direction::Sent),
            bytes_of(&events, Channel::Command, Direction::Sent),
        ]
        .concat()
    });
    assert_eq!(sent, Some(expected));
}

#[test]
fn the_reserve_carries_documentation_values_and_the_release_the_documentation_mac() {
    let passes = one_front();
    let expected = LocalTime::new(2026, 1, 1, 0, 0, 0).map(|time| {
        let reservation = Reservation {
            mac: HOST_MAC,
            host: HOST_ADDRESS,
            port: TRIGGER_PORT,
            time,
            host_id: HOST_ID,
            offset: 0,
        };
        [
            reserve(&reservation),
            Release::new(MacAddress::new(HOST_MAC)).encode().to_vec(),
        ]
        .to_vec()
    });
    let sent = redacted(&plain(&passes))
        .map(|(events, _)| bytes_of(&events, Channel::Control, Direction::Sent));
    assert_eq!(sent, expected);
}

#[test]
fn the_registered_identifier_gets_the_same_value_in_the_status_reply_and_the_reserve() {
    let passes = one_front();
    for (registered, expected) in [
        (PRIVATE_HOST_ID, HOST_ID),
        (UNREGISTERED, UNREGISTERED),
        (NO_IDENTIFIER, NO_IDENTIFIER),
    ] {
        let identifiers = redacted(&Raw {
            registered,
            ..plain(&passes)
        })
        .map(|(events, layout)| {
            (
                bytes_at(&events, layout.status_reply).to_vec(),
                bytes_at(&events, layout.reserve)
                    .get(RESERVE_HOST_ID..RESERVE_HOST_ID + 8)
                    .map(<[u8]>::to_vec),
            )
        });
        assert_eq!(
            identifiers,
            Some((status(expected), Some(expected.to_vec())))
        );
    }
}

#[test]
fn image_bytes_become_a_synthetic_image_with_the_original_frame_size() {
    let passes = one_front();
    let observed = redacted(&plain(&passes)).and_then(|(events, layout)| {
        let (header, payload) = *layout.chunks.first()?.first()?;
        let payload = bytes_at(&events, payload);
        Some((
            declared_payload(bytes_at(&events, header)) == Some(payload.len()),
            payload.get(..PREFIX).map(<[u8]>::to_vec),
            frame_size(payload.get(PREFIX..).unwrap_or_default()),
            payload.ends_with(&[0xFF, 0xD9]),
            payload
                .windows(PRIVATE_SERIAL.len())
                .any(|window| window == PRIVATE_SERIAL),
        ))
    });
    assert_eq!(
        observed,
        Some((true, Some([0x00, 0x00].to_vec()), Ok(FRONT), true, false))
    );
}

#[test]
fn every_chunk_of_a_side_stays_non_empty_and_the_chunks_join_into_one_image() {
    let passes = [pass(Side::Front, FRONT, 3)];
    let observed = redacted(&plain(&passes)).and_then(|(events, layout)| {
        let chunks = layout.chunks.first()?;
        let shapes: Vec<(bool, bool, Option<u8>)> = chunks
            .iter()
            .map(|(header, payload)| {
                let payload = bytes_at(&events, *payload);
                (
                    declared_payload(bytes_at(&events, *header)) == Some(payload.len()),
                    payload.len() > PREFIX,
                    payload.get(1).copied(),
                )
            })
            .collect();
        Some((shapes, frame_size(&joined(&events, chunks))))
    });
    assert_eq!(
        observed,
        Some((
            [
                (true, true, Some(0)),
                (true, true, Some(1)),
                (true, true, Some(2))
            ]
            .to_vec(),
            Ok(FRONT)
        ))
    );
}

#[test]
fn the_front_and_the_back_each_get_a_synthetic_image_of_their_own_size() {
    let passes = [pass(Side::Front, FRONT, 1), pass(Side::Back, BACK, 2)];
    let sizes = redacted(&plain(&passes)).map(|(events, layout)| {
        layout
            .chunks
            .iter()
            .map(|chunks| frame_size(&joined(&events, chunks)))
            .collect::<Vec<_>>()
    });
    assert_eq!(sizes, Some([Ok(FRONT), Ok(BACK)].to_vec()));
}

#[test]
fn consecutive_sides_with_the_same_page_byte_get_separate_images() {
    let passes = [pass(Side::Front, FRONT, 1), pass(Side::Front, BACK, 1)];
    let sizes = redacted(&plain(&passes)).map(|(events, layout)| {
        layout
            .chunks
            .iter()
            .map(|chunks| frame_size(&joined(&events, chunks)))
            .collect::<Vec<_>>()
    });
    assert_eq!(sizes, Some([Ok(FRONT), Ok(BACK)].to_vec()));
}

#[test]
fn the_sense_after_each_side_describes_the_synthetic_payload_that_it_follows() {
    let passes = [pass(Side::Front, FRONT, 2), pass(Side::Back, BACK, 1)];
    let residues = redacted(&plain(&passes)).map(|(events, layout)| {
        layout
            .chunks
            .iter()
            .zip(&layout.senses)
            .map(|(chunks, sense)| {
                let last = chunks
                    .last()
                    .map_or(0, |(_, payload)| bytes_at(&events, *payload).len());
                let information = bytes_at(&events, *sense)
                    .get(INFORMATION..)
                    .and_then(<[u8]>::first_chunk::<4>)
                    .map(|bytes| u32::from_be_bytes(*bytes));
                (
                    information,
                    u32::try_from(last)
                        .ok()
                        .and_then(|last| READ_LENGTH.checked_sub(last)),
                )
            })
            .collect::<Vec<_>>()
    });
    assert!(residues.is_some_and(|residues| {
        residues.len() == 2
            && residues
                .iter()
                .all(|(information, expected)| information.is_some() && information == expected)
    }));
}

#[test]
fn a_busy_reply_to_an_image_read_keeps_its_short_payload() {
    let passes = one_front();
    let busy = [0x5A_u8; BUSY_PAYLOAD];
    let kept = redacted(&Raw {
        busy: Some(&busy),
        ..plain(&passes)
    })
    .and_then(|(events, layout)| Some(bytes_at(&events, layout.busy?).to_vec()));
    assert_eq!(kept, Some(busy.to_vec()));
}

#[test]
fn a_longer_payload_beside_a_busy_status_is_refused() {
    let passes = one_front();
    let busy = [0x5A_u8; 100];
    let outcome = refused(
        &Raw {
            busy: Some(&busy),
            ..plain(&passes)
        },
        |_, _| {},
    );
    assert_eq!(
        outcome.map(|(result, layout)| (result.err(), layout.busy)),
        Some((Some(RedactError::UnexpectedEvent { index: 15 }), Some(15)))
    );
}

#[test]
fn an_event_of_unknown_shape_is_refused_with_its_index() {
    let passes = one_front();
    let outcome = refused(&plain(&passes), |events, layout| {
        events.insert(
            layout.reserve.saturating_add(1),
            event(Channel::Control, Direction::Received, &[0; 7]),
        );
    });
    assert_eq!(
        outcome.map(|(result, layout)| (result.err(), layout.reserve)),
        Some((Some(RedactError::UnexpectedEvent { index: 8 }), 7))
    );
}

#[test]
fn a_control_reply_of_unknown_length_is_refused() {
    let passes = one_front();
    let outcome = refused(&plain(&passes), |events, layout| {
        if let Some(reply) = events.get_mut(layout.reserve.saturating_add(1)) {
            reply.bytes = control_reply(24);
        }
    });
    assert_eq!(
        outcome.map(|(result, _)| result.err()),
        Some(Some(RedactError::UnexpectedEvent { index: 8 }))
    );
}

#[test]
fn a_reply_that_differs_from_its_declared_length_is_refused() {
    let passes = one_front();
    let outcome = refused(&plain(&passes), |events, layout| {
        if let Some(reply) = events.get_mut(layout.status_reply) {
            reply.bytes.pop();
        }
    });
    assert_eq!(
        outcome.map(|(result, layout)| (result.err(), layout.status_reply)),
        Some((Some(RedactError::UnexpectedEvent { index: 6 }), 6))
    );
}

#[test]
fn a_trace_that_ends_inside_an_exchange_is_refused() {
    let passes = one_front();
    let outcome = refused(&plain(&passes), |events, layout| {
        events.truncate(layout.status_reply);
    });
    assert_eq!(
        outcome.map(|(result, _)| result.err()),
        Some(Some(RedactError::Incomplete))
    );
}

#[test]
fn an_identifier_left_in_another_field_is_refused() {
    let passes = one_front();
    let mut busy = [0_u8; BUSY_PAYLOAD];
    put(&mut busy, 4, PRIVATE_SERIAL);
    let outcome = refused(
        &Raw {
            busy: Some(&busy),
            ..plain(&passes)
        },
        |_, _| {},
    );
    assert_eq!(
        outcome.map(|(result, _)| result.err()),
        Some(Some(RedactError::IdentifierSurvived { index: 15 }))
    );
}

#[test]
fn a_space_padded_serial_is_searched_without_its_padding() {
    let passes = one_front();
    let mut busy = [0_u8; BUSY_PAYLOAD];
    put(&mut busy, 4, PRIVATE_SERIAL);
    let outcome = refused(
        &Raw {
            serial: PADDED_SERIAL,
            busy: Some(&busy),
            ..plain(&passes)
        },
        |_, _| {},
    );
    assert_eq!(
        outcome.map(|(result, _)| result.err()),
        Some(Some(RedactError::IdentifierSurvived { index: 15 }))
    );
}

fn keepalive_reply(marker: [u8; 2]) -> Vec<u8> {
    let mut reply = b"VENS".to_vec();
    reply.extend_from_slice(&marker);
    reply.resize(12, 0);
    reply
}

fn with_keepalive(events: &mut Vec<Event>, after: usize, reply: &[u8]) {
    let probe = Probe::keepalive(
        Ipv4Address::new(PRIVATE_HOST),
        MacAddress::new(PRIVATE_HOST_MAC),
    )
    .encode();
    let exchange = [
        event(Channel::Discovery, Direction::Sent, &probe),
        event(Channel::Discovery, Direction::Received, reply),
        event(Channel::Discovery, Direction::Received, reply),
        event(Channel::Discovery, Direction::Received, reply),
    ];
    for (offset, keepalive) in exchange.into_iter().enumerate() {
        events.insert(after.saturating_add(offset), keepalive);
    }
}

#[test]
fn a_keepalive_probe_gets_documentation_values_and_its_replies_stay() {
    let passes = one_front();
    let reply = keepalive_reply([0x80, 0x01]);
    let outcome = refused(&plain(&passes), |events, layout| {
        with_keepalive(events, layout.reserve.saturating_add(2), &reply);
    });
    let expected_probe =
        Probe::keepalive(Ipv4Address::new(HOST_ADDRESS), MacAddress::new(HOST_MAC))
            .encode()
            .to_vec();
    assert_eq!(
        outcome.and_then(|(result, _)| result.ok()).map(|events| {
            bytes_of(&events, Channel::Discovery, Direction::Sent)
                .into_iter()
                .chain(
                    bytes_of(&events, Channel::Discovery, Direction::Received)
                        .into_iter()
                        .skip(1),
                )
                .collect::<Vec<_>>()
        }),
        Some(
            [
                Probe::search(Ipv4Address::new(HOST_ADDRESS), MacAddress::new(HOST_MAC))
                    .encode()
                    .to_vec(),
                expected_probe,
                reply.clone(),
                reply.clone(),
                reply,
            ]
            .to_vec()
        )
    );
}

#[test]
fn a_short_discovery_datagram_without_the_keepalive_marker_is_refused() {
    let passes = one_front();
    let outcome = refused(&plain(&passes), |events, layout| {
        with_keepalive(
            events,
            layout.reserve.saturating_add(2),
            &keepalive_reply([0x12, 0x34]),
        );
    });
    assert_eq!(
        outcome.map(|(result, layout)| (result.err(), layout.reserve)),
        Some((Some(RedactError::UnexpectedEvent { index: 10 }), 7))
    );
}

#[test]
fn a_side_without_a_frame_header_is_refused() {
    let passes = [Pass {
        side: Side::Front,
        chunks: [[0xFF, 0xD8, 0x12, 0x34, 0xFF, 0xD9].to_vec()].to_vec(),
    }];
    let outcome = refused(&plain(&passes), |_, _| {});
    assert_eq!(
        outcome.map(|(result, layout)| (
            result.err(),
            layout
                .chunks
                .first()
                .and_then(|chunks| chunks.first())
                .map(|(_, payload)| *payload)
        )),
        Some((
            Some(RedactError::MissingFrameHeader { index: 15 }),
            Some(15)
        ))
    );
}
