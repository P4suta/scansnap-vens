use crate::wire::field;

const START_OF_IMAGE: [u8; 2] = [0xFF, 0xD8];
const MARKER_PREFIX: u8 = 0xFF;
const FIRST_FRAME_MARKER: u8 = 0xC0;
const LAST_FRAME_MARKER: u8 = 0xC2;
const START_OF_SCAN: u8 = 0xDA;
const END_OF_IMAGE: u8 = 0xD9;
const MARKER_LENGTH: usize = 2;
const LENGTH_FIELD: usize = 2;
const HEIGHT_IN_FRAME: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JpegError {
    NotJpeg,
    Truncated,
    InvalidSegment { offset: usize },
    NoFrameHeader,
}

fn height_offset(image: &[u8]) -> Result<usize, JpegError> {
    if field(image, 0) != Some(START_OF_IMAGE) {
        return Err(JpegError::NotJpeg);
    }
    let mut offset = START_OF_IMAGE.len();
    loop {
        let [prefix, marker] = field(image, offset).ok_or(JpegError::Truncated)?;
        if prefix != MARKER_PREFIX {
            return Err(JpegError::InvalidSegment { offset });
        }
        match marker {
            FIRST_FRAME_MARKER..=LAST_FRAME_MARKER => {
                return offset
                    .checked_add(HEIGHT_IN_FRAME)
                    .ok_or(JpegError::Truncated);
            }
            START_OF_SCAN | END_OF_IMAGE => return Err(JpegError::NoFrameHeader),
            _ => {}
        }
        let body = offset
            .checked_add(MARKER_LENGTH)
            .ok_or(JpegError::Truncated)?;
        let length = field(image, body)
            .map(u16::from_be_bytes)
            .map(usize::from)
            .ok_or(JpegError::Truncated)?;
        if length < LENGTH_FIELD {
            return Err(JpegError::InvalidSegment { offset });
        }
        offset = body.checked_add(length).ok_or(JpegError::Truncated)?;
    }
}

/// Writes `height` into the frame header of a JPEG image.
///
/// # Errors
///
/// Refuses bytes that don't start an image, end before the height, break the marker segment structure, or reach the scan without a frame header.
pub fn set_height(image: &mut [u8], height: u16) -> Result<(), JpegError> {
    let offset = height_offset(image)?;
    let target = image
        .get_mut(offset..)
        .and_then(|rest| rest.first_chunk_mut::<2>())
        .ok_or(JpegError::Truncated)?;
    *target = height.to_be_bytes();
    Ok(())
}
