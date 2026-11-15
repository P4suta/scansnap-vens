//! Layouts follow sections 8.3 and 8.4 of the ix1300-wifi protocol document, both marked as verified on hardware.
//! Section 8.4 gives offsets and values, and section 4 gives the byte order.

use vens_core::image::{Chunk, ChunkError, PixelSize, PixelSizeError};

fn pixel_size() -> [u8; 32] {
    let mut data = [0_u8; 32];
    let fields = [(0, 2592_u32), (4, 10576), (8, 2592), (12, 9856)];
    for (offset, value) in fields {
        data.iter_mut()
            .skip(offset)
            .zip(value.to_be_bytes())
            .for_each(|(slot, byte)| *slot = byte);
    }
    data
}

#[test]
fn an_image_reply_splits_into_its_prefix_and_its_image_bytes() {
    assert_eq!(
        Chunk::split(&[0x02, 0x05, 0xFF, 0xD8, 0xAA]),
        Ok(Chunk {
            page: 2,
            sequence: 5,
            data: &[0xFF, 0xD8, 0xAA],
        })
    );
}

#[test]
fn an_image_reply_may_carry_only_its_prefix() {
    assert_eq!(
        Chunk::split(&[0x03, 0x00]),
        Ok(Chunk {
            page: 3,
            sequence: 0,
            data: &[],
        })
    );
}

#[test]
fn an_image_reply_shorter_than_its_prefix_is_refused() {
    assert_eq!(
        Chunk::split(&[0x03]),
        Err(ChunkError::Truncated {
            expected: 2,
            actual: 1,
        })
    );
}

#[test]
fn a_pixel_size_reply_yields_the_width_and_both_heights() {
    assert_eq!(
        PixelSize::decode(&pixel_size()).map(|size| (
            size.width(),
            size.padded_height(),
            size.real_height()
        )),
        Ok((2592, 10576, 9856))
    );
}

#[test]
fn a_pixel_size_reply_may_start_with_the_page_prefix() {
    let mut prefixed = [0_u8; 34];
    prefixed
        .iter_mut()
        .skip(2)
        .zip(pixel_size())
        .for_each(|(slot, byte)| *slot = byte);
    assert_eq!(
        PixelSize::decode(&prefixed).map(|size| size.real_height()),
        Ok(9856)
    );
}

#[test]
fn a_pixel_size_reply_of_another_length_is_refused() {
    for length in [0, 31, 33, 35] {
        let data = [0_u8; 35];
        assert_eq!(
            PixelSize::decode(data.get(..length).unwrap_or_default()).err(),
            Some(PixelSizeError::UnexpectedLength { actual: length })
        );
    }
}
