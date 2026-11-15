//! The layout and the paper rules follow section 2.1 of the ix1300-wifi protocol document, marked as verified on hardware.

use vens_core::{
    HostId,
    status::{HardwareStatus, PaperPath, StatusError},
};

const LENGTH: usize = 0x30;
const HOST_ID: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];

fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

fn status(feed: u8, chute: u8, host_id: [u8; 8]) -> [u8; LENGTH] {
    let mut frame = [0_u8; LENGTH];
    put(&mut frame, 2, &[feed, chute]);
    put(&mut frame, 0x20, &host_id);
    frame
}

fn paper(feed: u8, chute: u8) -> Result<PaperPath, StatusError> {
    HardwareStatus::decode(&status(feed, chute, HOST_ID)).map(|status| status.paper())
}

#[test]
fn paper_in_the_top_chute_wins_over_the_other_sensors() {
    assert_eq!(paper(0x00, 0x00), Ok(PaperPath::TopChute));
    assert_eq!(paper(0x80, 0x01), Ok(PaperPath::TopChute));
}

#[test]
fn an_empty_chute_with_paper_at_the_feed_sensor_is_the_front_slot() {
    assert_eq!(paper(0x80, 0x81), Ok(PaperPath::FrontSlot));
}

#[test]
fn an_empty_chute_without_paper_at_the_feed_sensor_has_no_paper() {
    assert_eq!(paper(0x00, 0x80), Ok(PaperPath::Absent));
    assert_eq!(paper(0x00, 0x81), Ok(PaperPath::Absent));
}

#[test]
fn the_exit_sensor_is_reported_on_its_own() {
    assert_eq!(
        HardwareStatus::decode(&status(0x00, 0x81, HOST_ID)).map(|status| status.exit_sensor()),
        Ok(true)
    );
    assert_eq!(
        HardwareStatus::decode(&status(0x00, 0x80, HOST_ID)).map(|status| status.exit_sensor()),
        Ok(false)
    );
}

#[test]
fn the_registered_host_identifier_is_read_from_the_long_reply() {
    assert_eq!(
        HardwareStatus::decode(&status(0, 0, HOST_ID)).map(|status| status.registered_host()),
        Ok(Some(HostId::new(HOST_ID)))
    );
}

#[test]
fn an_identifier_of_all_ones_means_no_registered_host() {
    assert_eq!(
        HardwareStatus::decode(&status(0, 0, [0xFF; 8])).map(|status| status.registered_host()),
        Ok(None)
    );
}

#[test]
fn a_reply_shorter_than_the_long_form_is_refused() {
    let frame = status(0, 0, HOST_ID);
    assert_eq!(
        HardwareStatus::decode(frame.get(..0x2F).unwrap_or_default()).err(),
        Some(StatusError::Truncated {
            expected: LENGTH,
            actual: 0x2F,
        })
    );
}
