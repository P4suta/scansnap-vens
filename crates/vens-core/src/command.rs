use crate::{
    DecodeError, Ipv4Address, MacAddress,
    wire::{KEY, keyed_field, put},
};

const LENGTH: usize = 0;
const FRAME_KEY: usize = 4;

const REQUEST_LENGTH: usize = 64;
const REQUEST_HEADER: u32 = 64;
const REQUEST_CDB_COUNT: usize = 8;
const REQUEST_HOST_MAC: usize = 16;
const REQUEST_HOST: usize = 24;
const REQUEST_CDB_LENGTH: usize = 32;
const REQUEST_DATA_IN: usize = 36;
const REQUEST_DATA_OUT: usize = 40;
const REQUEST_CDB: usize = 48;
const ONE_CDB: u32 = 1;

const SIX_BYTES: u32 = 6;
const TEN_BYTES: u32 = 10;
const TWELVE_BYTES: u32 = 12;

const REPLY_LENGTH: usize = 40;
const REPLY_HEADER: u32 = 40;
const REPLY_FRAME_STATUS: usize = 8;
const REPLY_SCSI_STATUS: usize = 12;
const FRAME_ACCEPTED: u32 = 0;
const GOOD: u32 = 0;
const CHECK_CONDITION: u32 = 2;
const BUSY: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cdb {
    Six([u8; 6]),
    Ten([u8; 10]),
    Twelve([u8; 12]),
}

impl Cdb {
    #[must_use]
    pub const fn bytes(&self) -> &[u8] {
        match self {
            Self::Six(bytes) => bytes,
            Self::Ten(bytes) => bytes,
            Self::Twelve(bytes) => bytes,
        }
    }

    const fn length(&self) -> u32 {
        match self {
            Self::Six(_) => SIX_BYTES,
            Self::Ten(_) => TEN_BYTES,
            Self::Twelve(_) => TWELVE_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    mac: MacAddress,
    host: Ipv4Address,
    cdb: Cdb,
    data_in: u32,
    data_out: u16,
}

impl Request {
    #[must_use]
    pub const fn new(mac: MacAddress, host: Ipv4Address, cdb: Cdb) -> Self {
        Self {
            mac,
            host,
            cdb,
            data_in: 0,
            data_out: 0,
        }
    }

    #[must_use]
    pub const fn expecting(self, data_in: u32) -> Self {
        Self { data_in, ..self }
    }

    #[must_use]
    pub const fn sending(self, data_out: u16) -> Self {
        Self { data_out, ..self }
    }

    #[must_use]
    pub fn encode(self) -> [u8; REQUEST_LENGTH] {
        let data_out = u32::from(self.data_out);
        let total = REQUEST_HEADER.saturating_add(data_out);
        let mut frame = [0; REQUEST_LENGTH];
        put(&mut frame, LENGTH, &total.to_be_bytes());
        put(&mut frame, FRAME_KEY, &KEY);
        put(&mut frame, REQUEST_CDB_COUNT, &ONE_CDB.to_be_bytes());
        put(&mut frame, REQUEST_HOST_MAC, &self.mac.octets());
        put(&mut frame, REQUEST_HOST, &self.host.octets());
        put(
            &mut frame,
            REQUEST_CDB_LENGTH,
            &self.cdb.length().to_be_bytes(),
        );
        put(&mut frame, REQUEST_DATA_IN, &self.data_in.to_be_bytes());
        put(&mut frame, REQUEST_DATA_OUT, &data_out.to_be_bytes());
        put(&mut frame, REQUEST_CDB, self.cdb.bytes());
        frame
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScsiStatus {
    Good,
    CheckCondition,
    Busy,
    Unrecognized(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyHeader {
    status: ScsiStatus,
    payload_length: u32,
}

impl ReplyHeader {
    /// # Errors
    ///
    /// Refuses a header that is truncated, carries another key, reports a refused frame, or declares a length below its own size.
    pub fn decode(frame: &[u8]) -> Result<Self, DecodeError> {
        let frame_status = keyed_field(frame, REPLY_LENGTH, FRAME_KEY, REPLY_FRAME_STATUS)
            .map(u32::from_be_bytes)?;
        if frame_status != FRAME_ACCEPTED {
            return Err(DecodeError::FrameRefused {
                status: frame_status,
            });
        }
        let declared =
            keyed_field(frame, REPLY_LENGTH, FRAME_KEY, LENGTH).map(u32::from_be_bytes)?;
        let payload_length = declared
            .checked_sub(REPLY_HEADER)
            .ok_or(DecodeError::LengthBelowHeader { declared })?;
        let status = match keyed_field(frame, REPLY_LENGTH, FRAME_KEY, REPLY_SCSI_STATUS)
            .map(u32::from_be_bytes)?
        {
            GOOD => ScsiStatus::Good,
            CHECK_CONDITION => ScsiStatus::CheckCondition,
            BUSY => ScsiStatus::Busy,
            other => ScsiStatus::Unrecognized(other),
        };
        Ok(Self {
            status,
            payload_length,
        })
    }

    #[must_use]
    pub const fn status(&self) -> ScsiStatus {
        self.status
    }

    #[must_use]
    pub const fn payload_length(&self) -> u32 {
        self.payload_length
    }
}
