pub(crate) const KEY: [u8; 4] = *b"VENS";
pub(crate) const INTERFACE_VERSION: [u8; 2] = [0x00, 0x10];

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
