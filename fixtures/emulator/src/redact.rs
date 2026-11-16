//! Turns a raw hardware trace into a fixture that keeps every status, sense key, and request length.
//! Identifier fields receive documentation values, and each side's image becomes a synthetic one with the original frame size.
//! The read residue after each side then describes the synthetic image, so the fixture doesn't reveal the original image sizes.
//! The field offsets follow sections 2.1, 5.1 to 5.4, 7.1, 7.2, 8.3, and 8.10 of the ix1300-wifi protocol document.

use std::iter;

use vens_core::{
    discovery::KeepaliveReply,
    jpeg::{FrameSize, frame_size},
};
use vens_net::trace::{Channel, Direction, Event};

pub const HOST_ADDRESS: [u8; 4] = [192, 0, 2, 12];
pub const HOST_MAC: [u8; 6] = [0x02, 0x76, 0x65, 0x6E, 0x73, 0x01];
pub const OTHER_HOST_ADDRESS: [u8; 4] = [192, 0, 2, 20];
pub const SCANNER_ADDRESS: [u8; 4] = [192, 0, 2, 7];
pub const SCANNER_MAC: [u8; 6] = [0x00, 0x00, 0x5E, 0x00, 0x53, 0x07];
pub const HOST_ID: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
pub const DEVICE_NAME: &str = "iX1300-EXAMPLE001";
pub const SERIAL: &str = "EXAMPLE001";
pub const TRIGGER_PORT: u16 = 55265;
const RESERVE_TIME: [u8; 7] = [0x07, 0xEA, 1, 1, 0, 0, 0];
const NO_OFFSET: [u8; 4] = [0; 4];

const KEY: [u8; 4] = *b"VENS";
const KEY_FIELD: usize = 4;
const PROBE_LENGTH: usize = 32;
const PROBE_HOST: usize = 8;
const PROBE_MAC: usize = 12;
const SEARCH_REPLY_LENGTH: usize = 144;
const KEEPALIVE_REPLY_LENGTH: usize = 12;
const SEARCH_SCANNER_ADDRESS: usize = 16;
const SEARCH_SCANNER_MAC: usize = 28;
const SEARCH_DEVICE_NAME: usize = 40;
const DEVICE_NAME_LENGTH: usize = 64;
const SEARCH_LAST_OWNER_ADDRESS: usize = 120;
const SEARCH_LAST_OWNER_ID: usize = 124;
const SEARCH_SERIAL: usize = 132;
const SERIAL_LENGTH: usize = 12;
const GREETING_LENGTH: usize = 16;
const REQUEST_LENGTH: usize = 64;
const REQUEST_MAC: usize = 16;
const REQUEST_HOST: usize = 24;
const REQUEST_DATA_IN: usize = 36;
const REQUEST_DATA_OUT: usize = 40;
const REQUEST_CDB: usize = 48;
const REPLY_HEADER_LENGTH: usize = 40;
const REPLY_STATUS: usize = 12;
const RESERVE_LENGTH: usize = 128;
const RESERVE_REPLY_LENGTH: usize = 20;
const RELEASE_LENGTH: usize = 32;
const RELEASE_REPLY_LENGTH: usize = 16;
const OPERATION: usize = 8;
const RESERVE_OPERATION: [u8; 4] = [0, 0, 0, 0x11];
const RELEASE_OPERATION: [u8; 4] = [0, 0, 0, 0x12];
const CONTROL_MAC: usize = 16;
const RESERVE_HOST: usize = 44;
const RESERVE_TRIGGER: usize = 48;
const RESERVE_TIME_FIELD: usize = 100;
const RESERVE_HOST_ID: usize = 108;
const RESERVE_OFFSET: usize = 116;
const STATUS_HOST_ID: usize = 0x20;
const HARDWARE_STATUS: u8 = 0xC2;
const REQUEST_SENSE: u8 = 0x03;
const READ: u8 = 0x28;
const READ_IMAGE: u8 = 0x00;
const CDB_KIND: usize = 2;
const CDB_PAGE: usize = 10;
const GOOD: u32 = 0;
const CHECK_CONDITION: u32 = 2;
const BUSY: u32 = 8;
const IMAGE_PREFIX: usize = 2;
const BUSY_PAYLOAD: usize = 34;
const SENSE_INFORMATION: usize = 3;
const SHORTEST_IDENTIFIER: usize = 4;
const NO_ADDRESS: [u8; 4] = [0; 4];
const SYNTHETIC_COMMENT: &[u8] = b"synthetic image";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactError {
    /// The event at `index` doesn't have the shape that the protocol puts at its place.
    UnexpectedEvent { index: usize },
    /// The image that starts at `index` has no frame header to size the synthetic image.
    MissingFrameHeader { index: usize },
    /// An identifier from the raw trace still appears in the event at `index`.
    IdentifierSurvived { index: usize },
    /// The trace ends inside a command exchange.
    Incomplete,
}

#[derive(Debug, Clone, Copy)]
struct Request {
    opcode: u8,
    kind: u8,
    page: u8,
    data_in: u32,
}

#[derive(Debug, Clone, Copy)]
enum Expect {
    Greeting,
    Request,
    DataOut {
        length: usize,
        request: Request,
    },
    ReplyHeader {
        request: Request,
    },
    ReplyPayload {
        length: usize,
        status: u32,
        header: usize,
        request: Request,
    },
}

#[derive(Debug, Clone, Copy)]
struct Chunk {
    page: u8,
    header: usize,
    payload: usize,
    ends_side: bool,
    data_in: u32,
    sense: Option<usize>,
}

struct Redaction {
    events: Vec<Event>,
    identifiers: Vec<Vec<u8>>,
    command: Expect,
    chunks: Vec<Chunk>,
    awaiting_sense: Option<usize>,
}

fn field<const N: usize>(frame: &[u8], offset: usize) -> Option<[u8; N]> {
    frame.get(offset..)?.first_chunk::<N>().copied()
}

fn swap(frame: &mut [u8], offset: usize, replacement: &[u8]) -> Option<Vec<u8>> {
    let end = offset.checked_add(replacement.len())?;
    let target = frame.get_mut(offset..end)?;
    let original = target.to_vec();
    target.copy_from_slice(replacement);
    Some(original)
}

fn padded<const N: usize>(text: &str) -> [u8; N] {
    let mut field = [0; N];
    field
        .iter_mut()
        .zip(text.as_bytes())
        .for_each(|(slot, byte)| *slot = *byte);
    field
}

/// Returns a text field without its zero terminator and without trailing space padding.
fn text(field: &[u8]) -> Vec<u8> {
    let used: Vec<u8> = field
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect();
    let length = used
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |last| last.saturating_add(1));
    used.into_iter().take(length).collect()
}

fn identifying(bytes: &[u8]) -> bool {
    let documentation: [&[u8]; 8] = [
        &HOST_ADDRESS,
        &HOST_MAC,
        &OTHER_HOST_ADDRESS,
        &SCANNER_ADDRESS,
        &SCANNER_MAC,
        &HOST_ID,
        DEVICE_NAME.as_bytes(),
        SERIAL.as_bytes(),
    ];
    bytes.len() >= SHORTEST_IDENTIFIER
        && !bytes.iter().all(|byte| *byte == 0x00)
        && !bytes.iter().all(|byte| *byte == 0xFF)
        && !documentation.contains(&bytes)
}

fn synthetic_image(size: FrameSize, filler: usize) -> Vec<u8> {
    let comment_length =
        u16::try_from(SYNTHETIC_COMMENT.len().saturating_add(2)).unwrap_or(u16::MAX);
    [
        [0xFF, 0xD8, 0xFF, 0xFE].as_slice(),
        &comment_length.to_be_bytes(),
        SYNTHETIC_COMMENT,
        &[0xFF, 0xC0, 0x00, 0x0B, 0x08],
        &size.height.to_be_bytes(),
        &size.width.to_be_bytes(),
        &[0x01, 0x01, 0x11, 0x00],
        &[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00],
        &vec![0_u8; filler],
        &[0xFF, 0xD9],
    ]
    .concat()
}

/// Splits an image into `parts` non-empty pieces: the first takes all but one byte for each later piece.
fn split(image: &[u8], parts: usize) -> Vec<&[u8]> {
    let head = image.len().saturating_sub(parts.saturating_sub(1));
    image
        .split_at_checked(head)
        .map_or_else(Vec::new, |(first, rest)| {
            iter::once(first)
                .chain(rest.iter().map(std::slice::from_ref))
                .collect()
        })
}

impl Redaction {
    fn visit(&mut self, index: usize) -> Option<()> {
        match self.events.get(index)?.channel {
            Channel::Discovery => self.discovery(index),
            Channel::Control => self.control(index),
            Channel::Command => self.command(index),
        }
    }

    fn discovery(&mut self, index: usize) -> Option<()> {
        let event = self.events.get_mut(index)?;
        let direction = event.direction;
        let frame = event.bytes.as_mut_slice();
        if field(frame, 0) != Some(KEY) {
            return None;
        }
        match (direction, frame.len()) {
            (Direction::Sent, PROBE_LENGTH) => {
                self.identifiers
                    .push(swap(frame, PROBE_HOST, &HOST_ADDRESS)?);
                self.identifiers.push(swap(frame, PROBE_MAC, &HOST_MAC)?);
            }
            (Direction::Received, SEARCH_REPLY_LENGTH) => {
                self.identifiers
                    .push(swap(frame, SEARCH_SCANNER_ADDRESS, &SCANNER_ADDRESS)?);
                self.identifiers
                    .push(swap(frame, SEARCH_SCANNER_MAC, &SCANNER_MAC)?);
                let name = padded::<DEVICE_NAME_LENGTH>(DEVICE_NAME);
                self.identifiers
                    .push(text(&swap(frame, SEARCH_DEVICE_NAME, &name)?));
                let serial = padded::<SERIAL_LENGTH>(SERIAL);
                self.identifiers
                    .push(text(&swap(frame, SEARCH_SERIAL, &serial)?));
                if field(frame, SEARCH_LAST_OWNER_ADDRESS) != Some(NO_ADDRESS) {
                    self.identifiers.push(swap(
                        frame,
                        SEARCH_LAST_OWNER_ADDRESS,
                        &OTHER_HOST_ADDRESS,
                    )?);
                }
                let owner: [u8; 8] = field(frame, SEARCH_LAST_OWNER_ID)?;
                if identifying(&owner) {
                    self.identifiers
                        .push(swap(frame, SEARCH_LAST_OWNER_ID, &HOST_ID)?);
                }
            }
            (Direction::Received, KEEPALIVE_REPLY_LENGTH)
                if KeepaliveReply::decode(frame).is_ok() => {}
            _ => return None,
        }
        Some(())
    }

    fn control(&mut self, index: usize) -> Option<()> {
        let event = self.events.get_mut(index)?;
        let direction = event.direction;
        let frame = event.bytes.as_mut_slice();
        if field(frame, KEY_FIELD) != Some(KEY) {
            return None;
        }
        let operation = field(frame, OPERATION);
        let replies = [GREETING_LENGTH, RESERVE_REPLY_LENGTH, RELEASE_REPLY_LENGTH];
        match (direction, frame.len()) {
            (Direction::Received, length) if replies.contains(&length) => {}
            (Direction::Sent, RESERVE_LENGTH) if operation == Some(RESERVE_OPERATION) => {
                self.identifiers.push(swap(frame, CONTROL_MAC, &HOST_MAC)?);
                self.identifiers
                    .push(swap(frame, RESERVE_HOST, &HOST_ADDRESS)?);
                swap(
                    frame,
                    RESERVE_TRIGGER,
                    &u32::from(TRIGGER_PORT).to_be_bytes(),
                )?;
                swap(frame, RESERVE_TIME_FIELD, &RESERVE_TIME)?;
                swap(frame, RESERVE_OFFSET, &NO_OFFSET)?;
                let reserved: [u8; 8] = field(frame, RESERVE_HOST_ID)?;
                if identifying(&reserved) {
                    self.identifiers
                        .push(swap(frame, RESERVE_HOST_ID, &HOST_ID)?);
                }
            }
            (Direction::Sent, RELEASE_LENGTH) if operation == Some(RELEASE_OPERATION) => {
                self.identifiers.push(swap(frame, CONTROL_MAC, &HOST_MAC)?);
            }
            _ => return None,
        }
        Some(())
    }

    fn command(&mut self, index: usize) -> Option<()> {
        let event = self.events.get(index)?;
        let direction = event.direction;
        let length = event.bytes.len();
        let keyed = field(&event.bytes, KEY_FIELD) == Some(KEY);
        self.command = match (self.command, direction) {
            (Expect::Greeting, Direction::Received) if keyed && length == GREETING_LENGTH => {
                Expect::Request
            }
            (Expect::Request, Direction::Sent) if keyed && length == REQUEST_LENGTH => {
                self.request(index)?
            }
            (
                Expect::DataOut {
                    length: expected,
                    request,
                },
                Direction::Sent,
            ) if length == expected => Expect::ReplyHeader { request },
            (Expect::ReplyHeader { request }, Direction::Received)
                if keyed && length == REPLY_HEADER_LENGTH =>
            {
                self.reply_header(index, request)?
            }
            (
                Expect::ReplyPayload {
                    length: expected,
                    status,
                    header,
                    request,
                },
                Direction::Received,
            ) if length == expected => {
                self.reply_payload(index, status, header, request)?;
                Expect::Request
            }
            _ => return None,
        };
        Some(())
    }

    fn request(&mut self, index: usize) -> Option<Expect> {
        let frame = self.events.get_mut(index)?.bytes.as_mut_slice();
        self.identifiers.push(swap(frame, REQUEST_MAC, &HOST_MAC)?);
        self.identifiers
            .push(swap(frame, REQUEST_HOST, &HOST_ADDRESS)?);
        let cdb: [u8; 16] = field(frame, REQUEST_CDB)?;
        let request = Request {
            opcode: *cdb.first()?,
            kind: *cdb.get(CDB_KIND)?,
            page: *cdb.get(CDB_PAGE)?,
            data_in: u32::from_be_bytes(field(frame, REQUEST_DATA_IN)?),
        };
        let data_out = usize::try_from(u32::from_be_bytes(field(frame, REQUEST_DATA_OUT)?)).ok()?;
        Some(if data_out == 0 {
            Expect::ReplyHeader { request }
        } else {
            Expect::DataOut {
                length: data_out,
                request,
            }
        })
    }

    fn reply_header(&self, index: usize, request: Request) -> Option<Expect> {
        let frame = self.events.get(index)?.bytes.as_slice();
        let total = usize::try_from(u32::from_be_bytes(field(frame, 0)?)).ok()?;
        let payload = total.checked_sub(REPLY_HEADER_LENGTH)?;
        let status = u32::from_be_bytes(field(frame, REPLY_STATUS)?);
        Some(if payload == 0 {
            Expect::Request
        } else {
            Expect::ReplyPayload {
                length: payload,
                status,
                header: index,
                request,
            }
        })
    }

    fn reply_payload(
        &mut self,
        index: usize,
        status: u32,
        header: usize,
        request: Request,
    ) -> Option<()> {
        let awaiting = self.awaiting_sense.take();
        let frame = self.events.get_mut(index)?.bytes.as_mut_slice();
        let length = frame.len();
        if request.opcode == HARDWARE_STATUS {
            let registered: Option<[u8; 8]> = field(frame, STATUS_HOST_ID);
            if registered.is_some_and(|identifier| identifying(&identifier)) {
                self.identifiers
                    .push(swap(frame, STATUS_HOST_ID, &HOST_ID)?);
            }
        }
        if request.opcode == REQUEST_SENSE
            && let Some(chunk) = awaiting.and_then(|chunk| self.chunks.get_mut(chunk))
        {
            chunk.sense = Some(index);
        }
        if request.opcode == READ && request.kind == READ_IMAGE {
            match status {
                GOOD | CHECK_CONDITION if length > IMAGE_PREFIX => {
                    let ends_side = status == CHECK_CONDITION;
                    if ends_side {
                        self.awaiting_sense = Some(self.chunks.len());
                    }
                    self.chunks.push(Chunk {
                        page: request.page,
                        header,
                        payload: index,
                        ends_side,
                        data_in: request.data_in,
                        sense: None,
                    });
                }
                GOOD | CHECK_CONDITION => {}
                BUSY if length <= BUSY_PAYLOAD => {}
                _ => return None,
            }
        }
        Some(())
    }

    /// Groups consecutive chunks into sides: a side ends at a chunk with a check condition or at a change of page.
    fn sides(&self) -> Vec<Vec<Chunk>> {
        let mut sides: Vec<Vec<Chunk>> = Vec::new();
        let mut open = false;
        for chunk in &self.chunks {
            match sides.last_mut() {
                Some(side) if open && side.last().is_some_and(|last| last.page == chunk.page) => {
                    side.push(*chunk);
                }
                Some(_) | None => sides.push(Vec::from([*chunk])),
            }
            open = !chunk.ends_side;
        }
        sides
    }

    fn replace_images(&mut self) -> Result<(), RedactError> {
        for side in self.sides() {
            self.replace_side(&side)?;
        }
        Ok(())
    }

    fn replace_side(&mut self, side: &[Chunk]) -> Result<(), RedactError> {
        let first = side.first().map_or(0, |chunk| chunk.payload);
        let original: Vec<u8> = side
            .iter()
            .filter_map(|chunk| self.events.get(chunk.payload))
            .flat_map(|event| {
                event
                    .bytes
                    .get(IMAGE_PREFIX..)
                    .unwrap_or_default()
                    .iter()
                    .copied()
            })
            .collect();
        let frame =
            frame_size(&original).map_err(|_| RedactError::MissingFrameHeader { index: first })?;
        let image = synthetic_image(frame, side.len());
        for (chunk, part) in side.iter().zip(split(&image, side.len())) {
            let unexpected = RedactError::UnexpectedEvent {
                index: chunk.payload,
            };
            let payload = self.events.get_mut(chunk.payload).ok_or(unexpected)?;
            let prefix: Vec<u8> = payload.bytes.iter().copied().take(IMAGE_PREFIX).collect();
            payload.bytes = [prefix.as_slice(), part].concat();
            let length = u32::try_from(payload.bytes.len()).map_err(|_| unexpected)?;
            let total = length
                .checked_add(u32::try_from(REPLY_HEADER_LENGTH).map_err(|_| unexpected)?)
                .ok_or(unexpected)?;
            let header = self.events.get_mut(chunk.header).ok_or(unexpected)?;
            swap(&mut header.bytes, 0, &total.to_be_bytes()).ok_or(unexpected)?;
            if let Some(sense) = chunk.sense {
                let residue = chunk.data_in.checked_sub(length).ok_or(unexpected)?;
                let sense = self.events.get_mut(sense).ok_or(unexpected)?;
                swap(&mut sense.bytes, SENSE_INFORMATION, &residue.to_be_bytes())
                    .ok_or(unexpected)?;
            }
        }
        Ok(())
    }

    fn verify(&self) -> Result<(), RedactError> {
        let identifiers: Vec<&[u8]> = self
            .identifiers
            .iter()
            .map(Vec::as_slice)
            .filter(|identifier| identifying(identifier))
            .collect();
        for (index, event) in self.events.iter().enumerate() {
            let survived = identifiers.iter().any(|identifier| {
                event
                    .bytes
                    .windows(identifier.len())
                    .any(|window| window == *identifier)
            });
            if survived {
                return Err(RedactError::IdentifierSurvived { index });
            }
        }
        Ok(())
    }
}

/// Replaces the identifiers and the image bytes of a raw trace and keeps every other byte.
///
/// # Errors
///
/// Refuses an event whose shape the protocol doesn't put at its place, a trace that ends inside a command exchange, an image without a frame header, and any raw identifier that survives the replacement.
pub fn redact(raw: &[Event]) -> Result<Vec<Event>, RedactError> {
    let mut redaction = Redaction {
        events: raw.to_vec(),
        identifiers: Vec::new(),
        command: Expect::Greeting,
        chunks: Vec::new(),
        awaiting_sense: None,
    };
    for index in 0..raw.len() {
        redaction
            .visit(index)
            .ok_or(RedactError::UnexpectedEvent { index })?;
    }
    match redaction.command {
        Expect::Greeting | Expect::Request => {}
        Expect::DataOut { .. } | Expect::ReplyHeader { .. } | Expect::ReplyPayload { .. } => {
            return Err(RedactError::Incomplete);
        }
    }
    redaction.replace_images()?;
    redaction.verify()?;
    Ok(redaction.events)
}
