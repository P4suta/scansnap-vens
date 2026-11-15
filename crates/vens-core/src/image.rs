use crate::wire::field;

const PREFIX_LENGTH: usize = 2;
const PIXEL_SIZE_LENGTH: usize = 32;
const PREFIXED_PIXEL_SIZE_LENGTH: usize = 34;
const WIDTH: usize = 0;
const PADDED_HEIGHT: usize = 4;
const REAL_HEIGHT: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'reply> {
    pub page: u8,
    pub sequence: u8,
    pub data: &'reply [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkError {
    Truncated { expected: usize, actual: usize },
}

impl<'reply> Chunk<'reply> {
    /// # Errors
    ///
    /// Refuses a reply that ends before the page and sequence prefix.
    pub const fn split(reply: &'reply [u8]) -> Result<Self, ChunkError> {
        match reply {
            [page, sequence, data @ ..] => Ok(Self {
                page: *page,
                sequence: *sequence,
                data,
            }),
            [] | [_] => Err(ChunkError::Truncated {
                expected: PREFIX_LENGTH,
                actual: reply.len(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSize {
    width: u32,
    padded_height: u32,
    real_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelSizeError {
    UnexpectedLength { actual: usize },
}

impl PixelSize {
    /// # Errors
    ///
    /// Refuses a reply whose length is neither the bare form nor the form with the page prefix.
    pub fn decode(reply: &[u8]) -> Result<Self, PixelSizeError> {
        let unexpected = PixelSizeError::UnexpectedLength {
            actual: reply.len(),
        };
        let body = match reply.len() {
            PIXEL_SIZE_LENGTH => reply,
            PREFIXED_PIXEL_SIZE_LENGTH => reply.get(PREFIX_LENGTH..).ok_or(unexpected)?,
            _ => return Err(unexpected),
        };
        let number = |offset| {
            field(body, offset)
                .map(u32::from_be_bytes)
                .ok_or(unexpected)
        };
        Ok(Self {
            width: number(WIDTH)?,
            padded_height: number(PADDED_HEIGHT)?,
            real_height: number(REAL_HEIGHT)?,
        })
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn padded_height(&self) -> u32 {
        self.padded_height
    }

    #[must_use]
    pub const fn real_height(&self) -> u32 {
        self.real_height
    }
}
