use crate::{
    Ipv4Address, MacAddress,
    command::{Cdb, Request},
    wire::put,
};

const PRE_READ_DATA_OUT: u16 = 32;
const PARAMETERS_DATA_OUT: u16 = 80;
const START_JOB_DATA_OUT: u16 = 8;

const CANCEL_READ: u8 = 0xD8;
const PRE_READ: u8 = 0xE9;
const SCAN_PARAMETERS: u8 = 0xD4;
const HARDWARE_STATUS: u8 = 0xC2;
const START_JOB: u8 = 0xD5;
const NEXT_SHEET: u8 = 0xE0;
const END_JOB: u8 = 0xD6;
const READ: u8 = 0x28;
const REQUEST_SENSE: u8 = 0x03;

const PRE_READ_LENGTH: usize = 32;
const PRE_READ_LENGTH_FIELD: u8 = 0x20;
const PRE_READ_X_RESOLUTION: usize = 0;
const PRE_READ_Y_RESOLUTION: usize = 2;
const PRE_READ_WINDOW_WIDTH: usize = 4;
const PRE_READ_AMOUNT: usize = 8;
const PRE_READ_TRAILER: usize = 12;
const PRE_READ_TRAILER_VALUE: [u8; 2] = [0x05, 0x00];
const TOP_CHUTE_PRE_READ: u32 = 0x4350;

const PARAMETERS_LENGTH: usize = 80;
const PARAMETERS_LENGTH_FIELD: u8 = 0x50;
const TOP_CHUTE_AUTOMATIC_COLOR: [u8; 14] = [
    0x00, 0x03, 0x01, 0x01, 0x88, 0x00, 0xC1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00,
];
const PARAMETERS_TRAILER: usize = 31;
const PARAMETERS_TRAILER_VALUE: u8 = 0x30;
const WINDOW_FORMAT: usize = 33;
const WINDOW_X_RESOLUTION: usize = 34;
const WINDOW_Y_RESOLUTION: usize = 36;
const WINDOW_COMPOSITION: usize = 38;
const WINDOW_RESOLUTION_CLASS: usize = 39;
const WINDOW_WIDTH_FIELD: usize = 42;
const WINDOW_LENGTH_FIELD: usize = 46;
const WINDOW_MARKER: usize = 50;
const WINDOW_TRAILER: usize = 62;
const JPEG: u8 = 0x10;
const COLOR: u8 = 0x05;
const CLASS_300_DPI: [u8; 2] = [0x82, 0x0D];
const WINDOW_MARKER_VALUE: u8 = 0x04;
const WINDOW_TRAILER_VALUE: u8 = 0x01;

const RESOLUTION: u16 = 300;
const WINDOW_WIDTH: u32 = 0x2880;
const AUTOMATIC_LENGTH: u32 = 0xA543;

const HARDWARE_STATUS_LENGTH: u8 = 0x30;
const START_JOB_LENGTH: u8 = 0x08;
const SENSE_LENGTH: u8 = 0x12;
const IMAGE_CHUNK: u32 = 0x0030_0000;
const PIXEL_SIZE: u8 = 0x80;
const PIXEL_SIZE_LENGTH: u8 = 0x20;
const FRONT_WINDOW: u8 = 0x00;
const BACK_WINDOW: u8 = 0x80;
const SIDES_PER_SHEET: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl Side {
    const fn window(self) -> u8 {
        match self {
            Self::Front => FRONT_WINDOW,
            Self::Back => BACK_WINDOW,
        }
    }

    const fn page_offset(self) -> u8 {
        match self {
            Self::Front => 0,
            Self::Back => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetIndex(u8);

impl SheetIndex {
    pub const FIRST: Self = Self(0);

    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    const fn page(self, side: Side) -> u8 {
        self.0
            .wrapping_mul(SIDES_PER_SHEET)
            .wrapping_add(side.page_offset())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequence(u8);

impl Sequence {
    pub const FIRST: Self = Self(0);

    #[must_use]
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Payload {
    Empty,
    PreRead([u8; PRE_READ_LENGTH]),
    Parameters([u8; PARAMETERS_LENGTH]),
    StartJob([u8; 8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    cdb: Cdb,
    data_in: u32,
    payload: Payload,
}

impl Command {
    const fn bare(cdb: Cdb, data_in: u32) -> Self {
        Self {
            cdb,
            data_in,
            payload: Payload::Empty,
        }
    }

    #[must_use]
    pub const fn cancel_read() -> Self {
        Self::bare(Cdb::Six([CANCEL_READ, 0, 0, 0, 0, 0]), 0)
    }

    #[must_use]
    pub fn pre_read() -> Self {
        let mut payload = [0; PRE_READ_LENGTH];
        put(
            &mut payload,
            PRE_READ_X_RESOLUTION,
            &RESOLUTION.to_be_bytes(),
        );
        put(
            &mut payload,
            PRE_READ_Y_RESOLUTION,
            &RESOLUTION.to_be_bytes(),
        );
        put(
            &mut payload,
            PRE_READ_WINDOW_WIDTH,
            &WINDOW_WIDTH.to_be_bytes(),
        );
        put(
            &mut payload,
            PRE_READ_AMOUNT,
            &TOP_CHUTE_PRE_READ.to_be_bytes(),
        );
        put(&mut payload, PRE_READ_TRAILER, &PRE_READ_TRAILER_VALUE);
        Self {
            cdb: Cdb::Ten([PRE_READ, 0, 0, 0, 0, 0, 0, PRE_READ_LENGTH_FIELD, 0, 0]),
            data_in: 0,
            payload: Payload::PreRead(payload),
        }
    }

    #[must_use]
    pub fn scan_parameters() -> Self {
        let mut payload = [0; PARAMETERS_LENGTH];
        put(&mut payload, 0, &TOP_CHUTE_AUTOMATIC_COLOR);
        put(
            &mut payload,
            PARAMETERS_TRAILER,
            &[PARAMETERS_TRAILER_VALUE],
        );
        put(&mut payload, WINDOW_FORMAT, &[JPEG]);
        put(&mut payload, WINDOW_X_RESOLUTION, &RESOLUTION.to_be_bytes());
        put(&mut payload, WINDOW_Y_RESOLUTION, &RESOLUTION.to_be_bytes());
        put(&mut payload, WINDOW_COMPOSITION, &[COLOR]);
        put(&mut payload, WINDOW_RESOLUTION_CLASS, &CLASS_300_DPI);
        put(
            &mut payload,
            WINDOW_WIDTH_FIELD,
            &WINDOW_WIDTH.to_be_bytes(),
        );
        put(
            &mut payload,
            WINDOW_LENGTH_FIELD,
            &AUTOMATIC_LENGTH.to_be_bytes(),
        );
        put(&mut payload, WINDOW_MARKER, &[WINDOW_MARKER_VALUE]);
        put(&mut payload, WINDOW_TRAILER, &[WINDOW_TRAILER_VALUE]);
        Self {
            cdb: Cdb::Six([SCAN_PARAMETERS, 0, 0, 0, PARAMETERS_LENGTH_FIELD, 0]),
            data_in: 0,
            payload: Payload::Parameters(payload),
        }
    }

    #[must_use]
    pub fn hardware_status() -> Self {
        Self::bare(
            Cdb::Ten([
                HARDWARE_STATUS,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                HARDWARE_STATUS_LENGTH,
                0,
            ]),
            u32::from(HARDWARE_STATUS_LENGTH),
        )
    }

    #[must_use]
    pub fn start_job() -> Self {
        Self {
            cdb: Cdb::Six([START_JOB, 0, 0, 0, START_JOB_LENGTH, START_JOB_LENGTH]),
            data_in: u32::from(START_JOB_LENGTH),
            payload: Payload::StartJob([0; 8]),
        }
    }

    #[must_use]
    pub const fn next_sheet() -> Self {
        Self::bare(Cdb::Six([NEXT_SHEET, 0, 0, 0, 0, 0]), 0)
    }

    #[must_use]
    pub const fn end_job() -> Self {
        Self::bare(Cdb::Six([END_JOB, 0, 0, 0, 0, 0]), 0)
    }

    #[must_use]
    pub const fn read_image(side: Side, sheet: SheetIndex, sequence: Sequence) -> Self {
        let [_, high, middle, low] = IMAGE_CHUNK.to_be_bytes();
        Self::bare(
            Cdb::Twelve([
                READ,
                0,
                0,
                0,
                0,
                side.window(),
                high,
                middle,
                low,
                0,
                sheet.page(side),
                sequence.0,
            ]),
            IMAGE_CHUNK,
        )
    }

    #[must_use]
    pub fn read_pixel_size(side: Side, sheet: SheetIndex) -> Self {
        Self::bare(
            Cdb::Twelve([
                READ,
                0,
                PIXEL_SIZE,
                0,
                0,
                side.window(),
                0,
                0,
                PIXEL_SIZE_LENGTH,
                0,
                sheet.page(side),
                0,
            ]),
            u32::from(PIXEL_SIZE_LENGTH),
        )
    }

    #[must_use]
    pub fn request_sense() -> Self {
        Self::bare(
            Cdb::Six([REQUEST_SENSE, 0, 0, 0, SENSE_LENGTH, 0]),
            u32::from(SENSE_LENGTH),
        )
    }

    #[must_use]
    pub const fn cdb(&self) -> &Cdb {
        &self.cdb
    }

    #[must_use]
    pub const fn data_in(&self) -> u32 {
        self.data_in
    }

    #[must_use]
    pub const fn request(&self, mac: MacAddress, host: Ipv4Address) -> Request {
        let data_out = match &self.payload {
            Payload::Empty => 0,
            Payload::PreRead(_) => PRE_READ_DATA_OUT,
            Payload::Parameters(_) => PARAMETERS_DATA_OUT,
            Payload::StartJob(_) => START_JOB_DATA_OUT,
        };
        Request::new(mac, host, self.cdb)
            .expecting(self.data_in)
            .sending(data_out)
    }

    #[must_use]
    pub const fn payload(&self) -> &[u8] {
        match &self.payload {
            Payload::Empty => &[],
            Payload::PreRead(bytes) => bytes,
            Payload::Parameters(bytes) => bytes,
            Payload::StartJob(bytes) => bytes,
        }
    }
}
