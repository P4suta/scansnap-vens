//! Layouts follow sections 4, 5.3, and 5.4 of the ix1300-wifi protocol document, all marked as verified on hardware.

use vens_core::{
    FrameError, HostId, Ipv4Address, MacAddress, Port,
    control::{
        Greeting, GreetingError, LocalTime, Release, ReleaseReply, Reserve, ReserveMode,
        ReserveRefusal, ReserveReply, UtcOffset,
    },
};

const HOST: Ipv4Address = Ipv4Address::new([192, 0, 2, 10]);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0, 0, 0, 0, 0x01]);
const HOST_ID: HostId = HostId::new([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
const TRIGGER_PORT: Port = Port::new(55265);
const RESERVE_LENGTH: usize = 128;

fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

fn reserve(mode: ReserveMode) -> Option<Reserve> {
    Some(Reserve {
        mode,
        mac: HOST_MAC,
        host: HOST,
        trigger_port: TRIGGER_PORT,
        time: LocalTime::new(2026, 11, 16, 21, 5, 9)?,
        host_id: HOST_ID,
        offset: UtcOffset::from_seconds(32_400),
    })
}

fn documented_reserve() -> [u8; RESERVE_LENGTH] {
    let mut frame = [0_u8; RESERVE_LENGTH];
    put(&mut frame, 0, &128_u32.to_be_bytes());
    put(&mut frame, 4, b"VENS");
    put(&mut frame, 8, &0x11_u32.to_be_bytes());
    put(&mut frame, 16, &[0x02, 0, 0, 0, 0, 0x01]);
    put(&mut frame, 32, &[0x00, 0x10]);
    put(&mut frame, 34, &[30]);
    put(&mut frame, 44, &[192, 0, 2, 10]);
    put(&mut frame, 48, &55265_u32.to_be_bytes());
    put(&mut frame, 100, &[0x07, 0xEA, 11, 16, 21, 5, 9]);
    put(
        &mut frame,
        108,
        &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF],
    );
    put(&mut frame, 116, &32_400_i32.to_be_bytes());
    frame
}

fn reply<const N: usize>(result: i32) -> [u8; N] {
    let mut frame = [0_u8; N];
    put(&mut frame, 4, b"VENS");
    put(&mut frame, 8, &result.to_be_bytes());
    frame
}

#[test]
fn a_scan_reserve_matches_the_documented_layout() {
    assert_eq!(
        reserve(ReserveMode::Scan).map(Reserve::encode),
        Some(documented_reserve())
    );
}

#[test]
fn a_registration_reserve_differs_only_in_its_mode() {
    let mut expected = documented_reserve();
    put(&mut expected, 36, &2_u32.to_be_bytes());
    assert_eq!(
        reserve(ReserveMode::Registration).map(Reserve::encode),
        Some(expected)
    );
}

#[test]
fn a_time_gives_back_each_of_its_fields() {
    assert_eq!(
        LocalTime::new(2026, 11, 16, 3, 4, 5).map(|time| (
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            time.second()
        )),
        Some((2026, 11, 16, 3, 4, 5))
    );
}

#[test]
fn seconds_since_the_unix_epoch_convert_to_the_civil_date_and_time() {
    let known = [
        (0, LocalTime::new(1970, 1, 1, 0, 0, 0)),
        (86_399, LocalTime::new(1970, 1, 1, 23, 59, 59)),
        (951_782_400, LocalTime::new(2000, 2, 29, 0, 0, 0)),
        (1_700_000_000, LocalTime::new(2023, 11, 14, 22, 13, 20)),
        (4_102_444_800, LocalTime::new(2100, 1, 1, 0, 0, 0)),
        (4_107_542_399, LocalTime::new(2100, 2, 28, 23, 59, 59)),
        (4_107_542_400, LocalTime::new(2100, 3, 1, 0, 0, 0)),
        (2_005_949_145_599, LocalTime::new(65535, 12, 31, 23, 59, 59)),
    ];
    for (seconds, expected) in known {
        assert_eq!(LocalTime::from_unix_seconds(seconds), expected);
        assert!(expected.is_some());
    }
}

#[test]
fn a_time_beyond_the_sixteen_bit_year_is_refused() {
    assert_eq!(LocalTime::from_unix_seconds(u64::MAX), None);
    assert_eq!(LocalTime::from_unix_seconds(2_005_949_145_600), None);
}

#[test]
fn a_local_time_outside_the_calendar_is_refused() {
    assert_eq!(LocalTime::new(2026, 13, 1, 0, 0, 0), None);
    assert_eq!(LocalTime::new(2026, 0, 1, 0, 0, 0), None);
    assert_eq!(LocalTime::new(2026, 1, 32, 0, 0, 0), None);
    assert_eq!(LocalTime::new(2026, 1, 0, 0, 0, 0), None);
    assert_eq!(LocalTime::new(2026, 1, 1, 24, 0, 0), None);
    assert_eq!(LocalTime::new(2026, 1, 1, 0, 60, 0), None);
    assert_eq!(LocalTime::new(2026, 1, 1, 0, 0, 60), None);
}

#[test]
fn a_reserve_reply_reports_success_or_the_documented_refusal() {
    let outcomes = [
        (0, Ok(())),
        (-1, Err(ReserveRefusal::General)),
        (-2, Err(ReserveRefusal::PasswordNecessary)),
        (-3, Err(ReserveRefusal::WrongPassword)),
        (-4, Err(ReserveRefusal::Occupied)),
        (-5, Err(ReserveRefusal::UnsupportedVersion)),
        (-6, Err(ReserveRefusal::HostIdNotRegistered)),
        (-7, Err(ReserveRefusal::HostIdNotValid)),
        (-8, Err(ReserveRefusal::SequenceError)),
        (-9, Err(ReserveRefusal::UsbMode)),
        (-10, Err(ReserveRefusal::Unrecognized(-10))),
    ];
    for (result, outcome) in outcomes {
        assert_eq!(
            ReserveReply::decode(&reply::<20>(result)).map(ReserveReply::outcome),
            Ok(outcome)
        );
    }
}

#[test]
fn a_release_matches_the_documented_layout() {
    let mut expected = [0_u8; 32];
    put(&mut expected, 0, &32_u32.to_be_bytes());
    put(&mut expected, 4, b"VENS");
    put(&mut expected, 8, &0x12_u32.to_be_bytes());
    put(&mut expected, 16, &[0x02, 0, 0, 0, 0, 0x01]);
    assert_eq!(Release::new(HOST_MAC).encode(), expected);
}

#[test]
fn a_release_reply_reports_its_result() {
    assert_eq!(
        ReleaseReply::decode(&reply::<16>(0)).map(ReleaseReply::outcome),
        Ok(Ok(()))
    );
    assert_eq!(
        ReleaseReply::decode(&reply::<16>(-1)).map(ReleaseReply::outcome),
        Ok(Err(-1))
    );
}

#[test]
fn a_greeting_is_accepted_only_in_its_documented_form() {
    let mut greeting = [0_u8; 16];
    put(&mut greeting, 0, &0x10_u32.to_be_bytes());
    put(&mut greeting, 4, b"VENS");
    assert_eq!(Greeting::decode(&greeting), Ok(Greeting));
    assert_eq!(
        Greeting::decode(greeting.get(..15).unwrap_or_default()),
        Err(GreetingError::Frame(FrameError::Truncated {
            expected: 16,
            actual: 15,
        }))
    );
    put(&mut greeting, 8, &5_u32.to_be_bytes());
    assert_eq!(
        Greeting::decode(&greeting),
        Err(GreetingError::Refused { status: 5 })
    );
    put(&mut greeting, 4, b"SNEV");
    assert_eq!(
        Greeting::decode(&greeting),
        Err(GreetingError::Frame(FrameError::ForeignKey))
    );
}

#[test]
fn a_control_reply_with_a_foreign_key_or_short_length_is_refused() {
    let mut frame = reply::<20>(0);
    assert_eq!(
        ReserveReply::decode(frame.get(..19).unwrap_or_default()).err(),
        Some(FrameError::Truncated {
            expected: 20,
            actual: 19,
        })
    );
    put(&mut frame, 4, b"SNEV");
    assert_eq!(
        ReserveReply::decode(&frame).err(),
        Some(FrameError::ForeignKey)
    );
}
