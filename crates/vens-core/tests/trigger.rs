use std::num::NonZeroU8;

use vens_core::trigger::{ButtonEvent, RecentPresses};

fn event(code: u8, sequence: u8) -> [u8; 48] {
    let mut bytes = [0; 48];
    for (slot, value) in bytes.iter_mut().skip(4).zip(*b"VENS") {
        *slot = value;
    }
    for (slot, value) in bytes.iter_mut().skip(11).zip([code, 0, 0, 0, 0, sequence]) {
        *slot = value;
    }
    bytes
}

#[test]
fn reads_the_verified_button_and_power_off_events_from_section_six() {
    assert_eq!(
        ButtonEvent::decode(&event(1, 17)),
        NonZeroU8::new(17).map(ButtonEvent::Press)
    );
    assert_eq!(
        ButtonEvent::decode(&event(0x10, 17)),
        Some(ButtonEvent::PowerOff)
    );
    assert_eq!(ButtonEvent::decode(&event(0x20, 17)), None);
}

#[test]
fn ignores_the_wrong_size_key_event_and_sequence() {
    let mut foreign = event(1, 1);
    if let Some(byte) = foreign.get_mut(4) {
        *byte = 0;
    }
    let press = event(1, 1);
    let oversized: Vec<u8> = press.into_iter().chain([0]).collect();
    assert_eq!(
        ButtonEvent::decode(&press),
        NonZeroU8::new(1).map(ButtonEvent::Press)
    );
    assert_eq!(
        [
            ButtonEvent::decode(&foreign),
            ButtonEvent::decode(&event(1, 0)),
            ButtonEvent::decode(&event(2, 1)),
            ButtonEvent::decode(press.split_last().map_or(&[], |(_, shorter)| shorter)),
            ButtonEvent::decode(&oversized),
        ],
        [None; 5]
    );
}

#[test]
fn starts_once_per_sequence_despite_late_copies_and_accepts_wraparound() {
    let mut recent = RecentPresses::default();
    let decisions = [254, 254, 255, 254, 255, 1, 255, 1, 2]
        .map(|sequence| NonZeroU8::new(sequence).is_some_and(|number| recent.accept(number)));
    assert_eq!(
        decisions,
        [true, false, true, false, false, true, false, false, true]
    );
    for sequence in (3..=18).filter_map(NonZeroU8::new) {
        assert!(recent.accept(sequence));
    }
    assert!(NonZeroU8::new(254).is_some_and(|number| recent.accept(number)));
}
