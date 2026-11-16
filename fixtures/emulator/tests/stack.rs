//! The fixture holds one real duplex scan of a stack of three different sheets, recorded on the scanner and redacted.
//! The session must replay it in feed order and write each side at the height that the scanner reported for it.

use vens_core::{
    Ipv4Address, MacAddress,
    discovery::Probe,
    job::{Pause, Summary},
    jpeg::frame_size,
    scan::Side,
};
use vens_emulator::{
    redact::{HOST_ADDRESS, HOST_MAC},
    session::replay_session,
};
use vens_net::trace::{self, Channel, Direction, Event};

const FIXTURE: &str = include_str!("../traces/three-duplex-sheets.trace");
const FIRST_SIDE_BUSY_REPLIES: usize = 14;

fn events() -> Vec<Event> {
    trace::parse(FIXTURE).unwrap_or_default()
}

#[test]
fn the_fixture_parses() {
    assert_eq!(trace::parse(FIXTURE).map(|events| events.len()), Ok(163));
}

#[test]
fn the_session_replays_three_sheets_in_feed_order_at_their_recorded_heights() {
    let replayed = replay_session(&events(), HOST_ADDRESS);
    assert_eq!(
        replayed.map(|replayed| (
            replayed.outcome,
            replayed
                .images
                .iter()
                .map(|(sheet, side, image)| (
                    *sheet,
                    *side,
                    frame_size(image).map(|size| size.height)
                ))
                .collect::<Vec<_>>(),
            replayed.finished,
        )),
        Some((
            Ok(Summary { sheets: 3 }),
            [
                (0, Side::Front, Ok(3648)),
                (0, Side::Back, Ok(3648)),
                (1, Side::Front, Ok(3656)),
                (1, Side::Back, Ok(3656)),
                (2, Side::Front, Ok(3648)),
                (2, Side::Back, Ok(3648)),
            ]
            .to_vec(),
            (true, true),
        ))
    );
}

#[test]
fn only_the_first_side_waits_through_busy_replies() {
    let pauses = replay_session(&events(), HOST_ADDRESS).map(|replayed| replayed.pauses);
    let busy = pauses
        .as_deref()
        .map(|pauses| pauses.iter().filter(|pause| **pause == Pause::Busy).count());
    let first_busy_run = pauses.as_deref().map(|pauses| {
        pauses
            .iter()
            .take_while(|pause| **pause == Pause::Busy)
            .count()
    });
    assert_eq!(
        (busy, first_busy_run),
        (Some(FIRST_SIDE_BUSY_REPLIES), Some(FIRST_SIDE_BUSY_REPLIES))
    );
}

#[test]
fn the_session_keeps_the_recorded_order_across_both_connections() {
    let recorded: Vec<Event> = events()
        .into_iter()
        .filter(|event| event.channel != Channel::Discovery)
        .collect();
    assert_eq!(
        replay_session(&events(), HOST_ADDRESS).map(|replayed| replayed.order),
        Some(recorded)
    );
}

#[test]
fn the_fixture_holds_one_keepalive_round_with_documentation_values() {
    let keepalive = Probe::keepalive(Ipv4Address::new(HOST_ADDRESS), MacAddress::new(HOST_MAC))
        .encode()
        .to_vec();
    let mut answer = b"VENS".to_vec();
    answer.extend_from_slice(&[0x80, 0x01]);
    answer.resize(12, 0);
    let discovery: Vec<(Direction, Vec<u8>)> = events()
        .into_iter()
        .filter(|event| event.channel == Channel::Discovery)
        .skip(2)
        .map(|event| (event.direction, event.bytes))
        .collect();
    assert_eq!(
        discovery,
        [
            (Direction::Sent, keepalive),
            (Direction::Received, answer.clone()),
            (Direction::Received, answer.clone()),
            (Direction::Received, answer),
        ]
        .to_vec()
    );
}
