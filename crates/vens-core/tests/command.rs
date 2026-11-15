//! Layouts follow sections 7.1 and 7.2 of the ix1300-wifi protocol document, both marked as verified on hardware.

use vens_core::{
    DecodeError, Ipv4Address, MacAddress,
    command::{Cdb, ReplyHeader, Request, ScsiStatus},
};

const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);
const HARDWARE_STATUS: [u8; 10] = [0xC2, 0, 0, 0, 0, 0, 0, 0, 0x30, 0];
const SCAN_PARAMETERS: [u8; 6] = [0xD4, 0, 0, 0, 0x50, 0];

fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

fn reply(total: u32, frame_status: u32, scsi_status: u32) -> [u8; 40] {
    let mut frame = [0_u8; 40];
    put(&mut frame, 0, &total.to_be_bytes());
    put(&mut frame, 4, b"VENS");
    put(&mut frame, 8, &frame_status.to_be_bytes());
    put(&mut frame, 12, &scsi_status.to_be_bytes());
    frame
}

#[test]
fn a_request_without_data_out_matches_the_documented_header() {
    let mut expected = [0_u8; 64];
    put(&mut expected, 0, &64_u32.to_be_bytes());
    put(&mut expected, 4, b"VENS");
    put(&mut expected, 8, &1_u32.to_be_bytes());
    put(&mut expected, 16, &[0x02, 0, 0, 0, 0, 0x01]);
    put(&mut expected, 24, &[192, 0, 2, 10]);
    put(&mut expected, 32, &10_u32.to_be_bytes());
    put(&mut expected, 36, &0x30_u32.to_be_bytes());
    put(&mut expected, 48, &HARDWARE_STATUS);
    assert_eq!(
        Request::new(HOST_MAC, HOST, Cdb::Ten(HARDWARE_STATUS))
            .expecting(0x30)
            .encode(),
        expected
    );
}

#[test]
fn a_request_with_data_out_counts_it_in_the_total_length() {
    let mut expected = [0_u8; 64];
    put(&mut expected, 0, &144_u32.to_be_bytes());
    put(&mut expected, 4, b"VENS");
    put(&mut expected, 8, &1_u32.to_be_bytes());
    put(&mut expected, 16, &[0x02, 0, 0, 0, 0, 0x01]);
    put(&mut expected, 24, &[192, 0, 2, 10]);
    put(&mut expected, 32, &6_u32.to_be_bytes());
    put(&mut expected, 40, &0x50_u32.to_be_bytes());
    put(&mut expected, 48, &SCAN_PARAMETERS);
    assert_eq!(
        Request::new(HOST_MAC, HOST, Cdb::Six(SCAN_PARAMETERS))
            .sending(0x50)
            .encode(),
        expected
    );
}

#[test]
fn a_twelve_byte_command_block_keeps_its_length() {
    let read = [0x28, 0, 0, 0, 0, 0x80, 0x30, 0, 0, 0, 1, 0];
    let frame = Request::new(HOST_MAC, HOST, Cdb::Twelve(read))
        .expecting(0x0030_0000)
        .encode();
    assert_eq!(frame.get(32..36), Some(12_u32.to_be_bytes().as_slice()));
    assert_eq!(frame.get(48..60), Some(read.as_slice()));
}

#[test]
fn a_reply_header_yields_the_status_and_the_payload_length() {
    let statuses = [
        (0, ScsiStatus::Good),
        (2, ScsiStatus::CheckCondition),
        (8, ScsiStatus::Busy),
        (4, ScsiStatus::Unrecognized(4)),
    ];
    for (code, status) in statuses {
        assert_eq!(
            ReplyHeader::decode(&reply(40 + 0x30, 0, code))
                .map(|header| (header.status(), header.payload_length())),
            Ok((status, 0x30))
        );
    }
}

#[test]
fn a_reply_header_in_a_refused_frame_is_an_error() {
    assert_eq!(
        ReplyHeader::decode(&reply(40, 7, 0)).err(),
        Some(DecodeError::FrameRefused { status: 7 })
    );
}

#[test]
fn a_reply_header_shorter_than_its_own_header_is_an_error() {
    assert_eq!(
        ReplyHeader::decode(&reply(39, 0, 0)).err(),
        Some(DecodeError::LengthBelowHeader { declared: 39 })
    );
    let frame = reply(40, 0, 0);
    assert_eq!(
        ReplyHeader::decode(frame.get(..39).unwrap_or_default()).err(),
        Some(DecodeError::Truncated {
            expected: 40,
            actual: 39,
        })
    );
}

#[test]
fn a_reply_header_with_a_foreign_key_is_an_error() {
    let mut frame = reply(40, 0, 0);
    put(&mut frame, 4, b"SNEV");
    assert_eq!(
        ReplyHeader::decode(&frame).err(),
        Some(DecodeError::ForeignKey)
    );
}
