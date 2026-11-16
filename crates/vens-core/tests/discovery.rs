//! Layouts follow sections 5.1 and 5.2 of the ix1300-wifi protocol document, both marked as verified on hardware.

use vens_core::{
    FrameError, HostId, Ipv4Address, MacAddress, Port,
    discovery::{
        Availability, KeepaliveReply, KeepaliveReplyError, Password, Probe, SearchReply,
        SearchReplyError,
    },
};

const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);
const REPLY_LENGTH: usize = 144;

fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

fn reply() -> [u8; REPLY_LENGTH] {
    let mut frame = [0_u8; REPLY_LENGTH];
    put(&mut frame, 0, b"VENS");
    put(&mut frame, 16, &[192, 0, 2, 20]);
    put(&mut frame, 20, &53218_u32.to_be_bytes());
    put(&mut frame, 24, &53219_u32.to_be_bytes());
    put(&mut frame, 28, &[0x02, 0, 0, 0, 0, 0x02]);
    put(&mut frame, 40, b"iX1300-EXAMPLE");
    put(&mut frame, 104, b"ScanSnap iX1300");
    put(&mut frame, 120, &[192, 0, 2, 10]);
    put(
        &mut frame,
        124,
        &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF],
    );
    put(&mut frame, 132, b"EXAMPLE00001");
    frame
}

fn keepalive_reply() -> [u8; 12] {
    let mut frame = [0_u8; 12];
    put(&mut frame, 0, b"VENS");
    put(&mut frame, 4, &[0x80, 0x01]);
    frame
}

#[test]
fn a_keepalive_reply_is_recognized_by_its_marker() {
    assert_eq!(
        KeepaliveReply::decode(&keepalive_reply()),
        Ok(KeepaliveReply)
    );
}

#[test]
fn a_search_reply_is_not_a_keepalive_reply() {
    assert_eq!(
        KeepaliveReply::decode(&reply()),
        Err(KeepaliveReplyError::NotAKeepaliveReply)
    );
}

#[test]
fn a_short_or_foreign_keepalive_reply_is_refused() {
    let mut foreign = keepalive_reply();
    put(&mut foreign, 0, b"SNEV");
    assert_eq!(
        KeepaliveReply::decode(keepalive_reply().get(..11).unwrap_or_default()),
        Err(KeepaliveReplyError::Frame(FrameError::Truncated {
            expected: 12,
            actual: 11
        }))
    );
    assert_eq!(
        KeepaliveReply::decode(&foreign),
        Err(KeepaliveReplyError::Frame(FrameError::ForeignKey))
    );
}

#[test]
fn a_search_probe_matches_the_documented_layout() {
    assert_eq!(
        Probe::search(HOST, HOST_MAC).encode(),
        [
            0x56, 0x45, 0x4E, 0x53, 0, 0, 0, 0, 192, 0, 2, 10, 0x02, 0, 0, 0, 0, 0x01, 0, 0, 0, 0,
            0, 0, 0x00, 0x10, 0, 0, 0, 0, 0, 0,
        ]
    );
}

#[test]
fn a_keepalive_differs_from_a_search_only_in_its_kind() {
    let mut expected = Probe::search(HOST, HOST_MAC).encode();
    put(&mut expected, 4, &1_u32.to_be_bytes());
    assert_eq!(Probe::keepalive(HOST, HOST_MAC).encode(), expected);
}

#[test]
fn a_search_reply_yields_the_scanner_endpoints_and_identity() {
    let decoded = SearchReply::decode(&reply());
    assert_eq!(
        decoded.map(|reply| (
            reply.address(),
            reply.command_port(),
            reply.control_port(),
            reply.mac(),
            reply.password(),
            reply.availability(),
            reply.last_owner(),
        )),
        Ok((
            Ipv4Address::new([192, 0, 2, 20]),
            Port::new(53218),
            Port::new(53219),
            MacAddress::new([0x02, 0, 0, 0, 0, 0x02]),
            Password::NotSet,
            Availability::Ready,
            HostId::new([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]),
        ))
    );
}

#[test]
fn a_search_reply_exposes_its_text_fields_without_padding() {
    let decoded = SearchReply::decode(&reply());
    assert_eq!(
        decoded.as_ref().map(|reply| (
            reply.device_name().as_str(),
            reply.product_name().as_str(),
            reply.serial().as_str(),
        )),
        Ok((
            Some("iX1300-EXAMPLE"),
            Some("ScanSnap iX1300"),
            Some("EXAMPLE00001"),
        ))
    );
}

#[test]
fn a_busy_scanner_and_a_password_are_reported() {
    let mut frame = reply();
    put(&mut frame, 12, &1_u32.to_be_bytes());
    put(&mut frame, 38, &2_u16.to_be_bytes());
    assert_eq!(
        SearchReply::decode(&frame).map(|reply| (reply.password(), reply.availability())),
        Ok((Password::Required, Availability::Busy))
    );
}

#[test]
fn a_truncated_reply_is_refused() {
    let frame = reply();
    assert_eq!(
        SearchReply::decode(frame.get(..143).unwrap_or_default()).err(),
        Some(SearchReplyError::Frame(FrameError::Truncated {
            expected: REPLY_LENGTH,
            actual: 143,
        }))
    );
}

#[test]
fn a_reply_with_a_foreign_key_is_refused() {
    let mut frame = reply();
    put(&mut frame, 0, b"SNEV");
    assert_eq!(
        SearchReply::decode(&frame).err(),
        Some(SearchReplyError::Frame(FrameError::ForeignKey))
    );
}

#[test]
fn a_reply_marked_invalid_is_refused() {
    let mut frame = reply();
    put(&mut frame, 6, &[0xFF, 0xFF]);
    assert_eq!(
        SearchReply::decode(&frame).err(),
        Some(SearchReplyError::NotAReply)
    );
}

#[test]
fn a_port_beyond_sixteen_bits_is_refused() {
    let mut frame = reply();
    put(&mut frame, 20, &65536_u32.to_be_bytes());
    assert_eq!(
        SearchReply::decode(&frame).err(),
        Some(SearchReplyError::PortOutOfRange { value: 65536 })
    );
}
