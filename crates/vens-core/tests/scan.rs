//! Commands follow sections 8.1 to 8.4 of the ix1300-wifi protocol document, all marked as verified on hardware.
//! The settings are the tested ones: 300 dpi, automatic color, both sides, top chute.

use vens_core::scan::{Command, Sequence, SheetIndex, Side};

fn parts(command: &Command) -> (&[u8], u32, &[u8]) {
    (command.cdb().bytes(), command.data_in(), command.payload())
}

#[test]
fn the_job_opens_with_a_cancelled_read() {
    assert_eq!(
        parts(&Command::cancel_read()),
        ([0xD8, 0, 0, 0, 0, 0].as_slice(), 0, [].as_slice())
    );
}

#[test]
fn the_pre_read_settings_carry_the_tested_window() {
    let payload = [
        0x01, 0x2C, 0x01, 0x2C, 0x00, 0x00, 0x28, 0x80, 0x00, 0x00, 0x43, 0x50, 0x05, 0x00, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(
        parts(&Command::pre_read()),
        (
            [0xE9, 0, 0, 0, 0, 0, 0, 0x20, 0, 0].as_slice(),
            0,
            payload.as_slice()
        )
    );
}

#[test]
fn the_scan_parameters_carry_the_tested_settings_and_window() {
    let payload = [
        0x00, 0x03, 0x01, 0x01, 0x88, 0x00, 0xC1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x30, // parameters
        0x00, 0x10, 0x01, 0x2C, 0x01, 0x2C, 0x05, 0x82, 0x0D, 0x00, 0x00, 0x00, 0x28, 0x80, 0x00,
        0x00, 0xA5, 0x43, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, // window descriptor
    ];
    assert_eq!(
        parts(&Command::scan_parameters()),
        ([0xD4, 0, 0, 0, 0x50, 0].as_slice(), 0, payload.as_slice())
    );
}

#[test]
fn the_hardware_status_asks_for_the_reply_that_holds_the_host_identifier() {
    assert_eq!(
        parts(&Command::hardware_status()),
        (
            [0xC2, 0, 0, 0, 0, 0, 0, 0, 0x30, 0].as_slice(),
            0x30,
            [].as_slice()
        )
    );
}

#[test]
fn the_job_starts_with_eight_zero_bytes_and_reads_eight_back() {
    assert_eq!(
        parts(&Command::start_job()),
        (
            [0xD5, 0, 0, 0, 0x08, 0x08].as_slice(),
            8,
            [0_u8; 8].as_slice()
        )
    );
}

#[test]
fn sheets_advance_and_the_job_ends_without_data() {
    assert_eq!(
        parts(&Command::next_sheet()),
        ([0xE0, 0, 0, 0, 0, 0].as_slice(), 0, [].as_slice())
    );
    assert_eq!(
        parts(&Command::end_job()),
        ([0xD6, 0, 0, 0, 0, 0].as_slice(), 0, [].as_slice())
    );
}

#[test]
fn an_image_read_names_the_side_the_page_and_the_sequence() {
    assert_eq!(
        parts(&Command::read_image(
            Side::Front,
            SheetIndex::FIRST,
            Sequence::FIRST
        )),
        (
            [0x28, 0, 0, 0, 0, 0x00, 0x30, 0, 0, 0, 0, 0].as_slice(),
            0x0030_0000,
            [].as_slice()
        )
    );
    let second_sheet = SheetIndex::FIRST.next();
    let third_read = Sequence::FIRST.next().and_then(Sequence::next);
    assert_eq!(
        third_read
            .map(|sequence| Command::read_image(Side::Back, second_sheet, sequence))
            .as_ref()
            .map(parts),
        Some((
            [0x28, 0, 0, 0, 0, 0x80, 0x30, 0, 0, 0, 3, 2].as_slice(),
            0x0030_0000,
            [].as_slice()
        ))
    );
}

#[test]
fn a_pixel_size_read_names_the_side_and_the_page() {
    assert_eq!(
        parts(&Command::read_pixel_size(Side::Back, SheetIndex::FIRST)),
        (
            [0x28, 0, 0x80, 0, 0, 0x80, 0, 0, 0x20, 0, 1, 0].as_slice(),
            32,
            [].as_slice()
        )
    );
}

#[test]
fn the_page_identifier_wraps_at_one_byte() {
    let sheet = (0..128).fold(SheetIndex::FIRST, |sheet, _| sheet.next());
    assert_eq!(
        Command::read_pixel_size(Side::Back, sheet).cdb().bytes(),
        [0x28, 0, 0x80, 0, 0, 0x80, 0, 0, 0x20, 0, 1, 0]
    );
}

#[test]
fn a_sequence_stops_at_the_end_of_its_byte() {
    let last = (0..255).try_fold(Sequence::FIRST, |sequence, _| sequence.next());
    assert_eq!(last.and_then(Sequence::next), None);
}

#[test]
fn a_sense_request_reads_eighteen_bytes() {
    assert_eq!(
        parts(&Command::request_sense()),
        ([0x03, 0, 0, 0, 0x12, 0].as_slice(), 18, [].as_slice())
    );
}
