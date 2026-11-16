use core::fmt;

const TEXT_LENGTH: usize = 36;
const HYPHENS: [usize; 4] = [8, 13, 18, 23];
const BREAKS: [usize; 4] = [4, 6, 8, 10];

/// A 128-bit identifier in the hyphenated form of RFC 9562.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }

    /// Reads the hyphenated form in either letter case, and refuses any other text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != TEXT_LENGTH {
            return None;
        }
        let mut nibbles = [0_u8; 32];
        let mut digits = nibbles.iter_mut();
        for (index, byte) in text.bytes().enumerate() {
            if HYPHENS.contains(&index) {
                if byte != b'-' {
                    return None;
                }
            } else {
                let value = char::from(byte).to_digit(16)?;
                *digits.next()? = u8::try_from(value).ok()?;
            }
        }
        let mut bytes = [0_u8; 16];
        for (slot, [high, low]) in bytes.iter_mut().zip(nibbles.as_chunks::<2>().0) {
            *slot = high.wrapping_shl(4) | low;
        }
        Some(Self(bytes))
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if BREAKS.contains(&index) {
                formatter.write_str("-")?;
            }
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}
