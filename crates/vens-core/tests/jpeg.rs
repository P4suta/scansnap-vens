//! The correction follows section 8.4 of the ix1300-wifi protocol document.
//! The marker segments and the frame header follow annex B of ITU-T T.81.

use vens_core::jpeg::{FrameSize, JpegError, frame_size, set_height};

const HEIGHT_OFFSET: usize = 18;
const WIDTH_END: usize = 22;

fn image(frame_marker: u8) -> [u8; 34] {
    [
        0xFF,
        0xD8, // start of image
        0xFF,
        0xE0,
        0x00,
        0x04,
        0xAA,
        0xBB, // application segment
        0xFF,
        0xDB,
        0x00,
        0x03,
        0xCC, // quantization table
        0xFF,
        frame_marker,
        0x00,
        0x0B,
        0x08,
        0x29,
        0x50,
        0x0A,
        0x20,
        0x01,
        0x01,
        0x11,
        0x00, // frame header: 10576 lines, 2592 samples
        0xFF,
        0xDA,
        0x00,
        0x02, // start of scan
        0x12,
        0x34,
        0xFF,
        0xD9, // end of image
    ]
}

fn replaced(mut bytes: [u8; 34], offset: usize, values: &[u8]) -> [u8; 34] {
    bytes
        .iter_mut()
        .skip(offset)
        .zip(values)
        .for_each(|(slot, value)| *slot = *value);
    bytes
}

#[test]
fn the_real_height_replaces_the_padded_height_and_nothing_else() {
    for frame_marker in [0xC0, 0xC2] {
        let mut bytes = image(frame_marker);
        assert_eq!(set_height(&mut bytes, 9856), Ok(()));
        assert_eq!(
            bytes,
            replaced(image(frame_marker), HEIGHT_OFFSET, &[0x26, 0x80])
        );
    }
}

#[test]
fn bytes_that_do_not_start_an_image_are_refused() {
    let mut bytes = replaced(image(0xC0), 0, &[0x00, 0x00]);
    assert_eq!(set_height(&mut bytes, 9856), Err(JpegError::NotJpeg));
    assert_eq!(set_height(&mut [], 9856), Err(JpegError::NotJpeg));
}

#[test]
fn an_image_whose_scan_starts_before_a_frame_header_is_refused() {
    let mut bytes = replaced(image(0xC0), 14, &[0xDA]);
    assert_eq!(set_height(&mut bytes, 9856), Err(JpegError::NoFrameHeader));
}

#[test]
fn an_image_cut_before_the_height_is_refused_at_every_length() {
    let bytes = image(0xC0);
    for length in 2..HEIGHT_OFFSET + 2 {
        let mut cut = bytes;
        assert_eq!(
            set_height(cut.get_mut(..length).unwrap_or_default(), 9856),
            Err(JpegError::Truncated)
        );
    }
}

#[test]
fn a_segment_without_a_marker_prefix_is_refused_with_its_position() {
    let mut bytes = replaced(image(0xC0), 8, &[0x00]);
    assert_eq!(
        set_height(&mut bytes, 9856),
        Err(JpegError::InvalidSegment { offset: 8 })
    );
}

#[test]
fn the_frame_size_comes_from_the_frame_header() {
    for frame_marker in [0xC0, 0xC2] {
        assert_eq!(
            frame_size(&image(frame_marker)),
            Ok(FrameSize {
                width: 2592,
                height: 10576
            })
        );
    }
}

#[test]
fn the_frame_size_of_an_image_without_a_frame_header_is_refused() {
    assert_eq!(
        frame_size(&replaced(image(0xC0), 14, &[0xDA])),
        Err(JpegError::NoFrameHeader)
    );
    assert_eq!(frame_size(&[]), Err(JpegError::NotJpeg));
}

#[test]
fn the_frame_size_of_an_image_cut_before_the_width_is_refused_at_every_length() {
    let bytes = image(0xC0);
    for length in 2..WIDTH_END {
        assert_eq!(
            frame_size(bytes.get(..length).unwrap_or_default()),
            Err(JpegError::Truncated)
        );
    }
}

#[test]
fn a_segment_length_below_its_own_field_is_refused_with_its_position() {
    let mut bytes = replaced(image(0xC0), 4, &[0x00, 0x01]);
    assert_eq!(
        set_height(&mut bytes, 9856),
        Err(JpegError::InvalidSegment { offset: 2 })
    );
}
