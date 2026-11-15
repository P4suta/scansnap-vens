pub(crate) const KEY: [u8; 4] = *b"VENS";
pub(crate) const INTERFACE_VERSION: [u8; 2] = [0x00, 0x10];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Truncated { expected: usize, actual: usize },
    ForeignKey,
    NotAReply,
    PortOutOfRange { value: u32 },
}

pub(crate) fn put(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    frame
        .iter_mut()
        .skip(offset)
        .zip(bytes)
        .for_each(|(slot, byte)| *slot = *byte);
}

pub(crate) fn field<const N: usize>(frame: &[u8], offset: usize) -> Option<[u8; N]> {
    frame.get(offset..)?.first_chunk::<N>().copied()
}

pub(crate) fn keyed_field<const N: usize>(
    frame: &[u8],
    length: usize,
    key: usize,
    value: usize,
) -> Result<[u8; N], DecodeError> {
    let truncated = DecodeError::Truncated {
        expected: length,
        actual: frame.len(),
    };
    if frame.len() < length {
        return Err(truncated);
    }
    if field(frame, key) != Some(KEY) {
        return Err(DecodeError::ForeignKey);
    }
    field(frame, value).ok_or(truncated)
}
