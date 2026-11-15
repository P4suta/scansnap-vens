use core::ops::RangeInclusive;

use crate::{
    FrameError, HostId, Ipv4Address, MacAddress, Port,
    wire::{INTERFACE_VERSION, KEY, keyed_field, put},
};

const LENGTH: usize = 0;
const FRAME_KEY: usize = 4;
const COMMAND: usize = 8;
const RESULT: usize = 8;
const HOST_MAC: usize = 16;

const RESERVE_LENGTH: usize = 128;
const RESERVE_LENGTH_FIELD: [u8; 4] = 128_u32.to_be_bytes();
const RESERVE_COMMAND: u32 = 0x11;
const RESERVE_VERSION: usize = 32;
const RESERVE_TIMEOUT: usize = 34;
const RESERVE_MODE: usize = 36;
const RESERVE_HOST: usize = 44;
const RESERVE_TRIGGER_PORT: usize = 48;
const RESERVE_TIME: usize = 100;
const RESERVE_HOST_ID: usize = 108;
const RESERVE_OFFSET: usize = 116;
const RESERVE_REPLY_LENGTH: usize = 20;
const TIMEOUT_SECONDS: u8 = 30;
const SCAN_MODE: u32 = 0;
const REGISTRATION_MODE: u32 = 2;

const RELEASE_LENGTH: usize = 32;
const RELEASE_LENGTH_FIELD: [u8; 4] = 32_u32.to_be_bytes();
const RELEASE_COMMAND: u32 = 0x12;
const RELEASE_REPLY_LENGTH: usize = 16;

const GREETING_LENGTH: usize = 16;
const GREETING_STATUS: usize = 8;
const GREETING_ACCEPTED: u32 = 0;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;
const SECONDS_PER_DAY: u64 = 86_400;
const DAYS_BEFORE_EPOCH: u64 = 719_468;
const DAYS_PER_ERA: u64 = 146_097;
const LAST_DAY_OF_ERA: u64 = 146_096;
const DAYS_PER_CENTURY: u64 = 36_524;
const DAYS_PER_FOUR_YEARS: u64 = 1_460;
const DAYS_PER_YEAR: u64 = 365;
const DAYS_PER_FIVE_MONTHS: u64 = 153;
const YEARS_PER_ERA: u64 = 400;
const YEARS_PER_CENTURY: u64 = 100;
const YEARS_PER_LEAP: u64 = 4;
const MONTH_SCALE: u64 = 5;
const MONTH_BIAS: u64 = 2;
const MARCH: u64 = 3;
const MONTHS_FROM_MARCH_TO_DECEMBER: u64 = 10;

const MONTHS: RangeInclusive<u8> = 1..=12;
const DAYS: RangeInclusive<u8> = 1..=31;
const HOURS: RangeInclusive<u8> = 0..=23;
const SIXTIETHS: RangeInclusive<u8> = 0..=59;

const ACCEPTED: i32 = 0;
const GENERAL: i32 = -1;
const PASSWORD_NECESSARY: i32 = -2;
const WRONG_PASSWORD: i32 = -3;
const OCCUPIED: i32 = -4;
const UNSUPPORTED_VERSION: i32 = -5;
const HOST_ID_NOT_REGISTERED: i32 = -6;
const HOST_ID_NOT_VALID: i32 = -7;
const SEQUENCE_ERROR: i32 = -8;
const USB_MODE: i32 = -9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

impl LocalTime {
    #[must_use]
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        let valid = MONTHS.contains(&month)
            && DAYS.contains(&day)
            && HOURS.contains(&hour)
            && SIXTIETHS.contains(&minute)
            && SIXTIETHS.contains(&second);
        valid.then_some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }

    /// Converts seconds since the Unix epoch to the proleptic Gregorian date and the time of day.
    #[must_use]
    pub fn from_unix_seconds(seconds: u64) -> Option<Self> {
        let second_of_day = seconds % SECONDS_PER_DAY;
        let shifted = (seconds / SECONDS_PER_DAY).checked_add(DAYS_BEFORE_EPOCH)?;
        let era = shifted / DAYS_PER_ERA;
        let day_of_era = shifted % DAYS_PER_ERA;
        let year_of_era = day_of_era
            .checked_sub(day_of_era / DAYS_PER_FOUR_YEARS)?
            .checked_add(day_of_era / DAYS_PER_CENTURY)?
            .checked_sub(day_of_era / LAST_DAY_OF_ERA)?
            / DAYS_PER_YEAR;
        let year_start = DAYS_PER_YEAR
            .checked_mul(year_of_era)?
            .checked_add(year_of_era / YEARS_PER_LEAP)?
            .checked_sub(year_of_era / YEARS_PER_CENTURY)?;
        let day_of_year = day_of_era.checked_sub(year_start)?;
        let month_index = MONTH_SCALE
            .checked_mul(day_of_year)?
            .checked_add(MONTH_BIAS)?
            / DAYS_PER_FIVE_MONTHS;
        let month_start = DAYS_PER_FIVE_MONTHS
            .checked_mul(month_index)?
            .checked_add(MONTH_BIAS)?
            / MONTH_SCALE;
        let day = day_of_year.checked_sub(month_start)?.checked_add(1)?;
        let month = if month_index < MONTHS_FROM_MARCH_TO_DECEMBER {
            month_index.checked_add(MARCH)?
        } else {
            month_index
                .checked_sub(MONTHS_FROM_MARCH_TO_DECEMBER)?
                .checked_add(1)?
        };
        let year = era
            .checked_mul(YEARS_PER_ERA)?
            .checked_add(year_of_era)?
            .checked_add(u64::from(month < MARCH))?;
        Self::new(
            u16::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
            u8::try_from(second_of_day / SECONDS_PER_HOUR).ok()?,
            u8::try_from(second_of_day % SECONDS_PER_HOUR / SECONDS_PER_MINUTE).ok()?,
            u8::try_from(second_of_day % SECONDS_PER_MINUTE).ok()?,
        )
    }

    const fn encode(self) -> [u8; 7] {
        let [high, low] = self.year.to_be_bytes();
        [
            high,
            low,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UtcOffset(i32);

impl UtcOffset {
    #[must_use]
    pub const fn from_seconds(seconds: i32) -> Self {
        Self(seconds)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReserveMode {
    Scan,
    Registration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reserve {
    pub mode: ReserveMode,
    pub mac: MacAddress,
    pub host: Ipv4Address,
    pub trigger_port: Port,
    pub time: LocalTime,
    pub host_id: HostId,
    pub offset: UtcOffset,
}

impl Reserve {
    #[must_use]
    pub fn encode(self) -> [u8; RESERVE_LENGTH] {
        let mode = match self.mode {
            ReserveMode::Scan => SCAN_MODE,
            ReserveMode::Registration => REGISTRATION_MODE,
        };
        let mut frame = [0; RESERVE_LENGTH];
        put(&mut frame, LENGTH, &RESERVE_LENGTH_FIELD);
        put(&mut frame, FRAME_KEY, &KEY);
        put(&mut frame, COMMAND, &RESERVE_COMMAND.to_be_bytes());
        put(&mut frame, HOST_MAC, &self.mac.octets());
        put(&mut frame, RESERVE_VERSION, &INTERFACE_VERSION);
        put(&mut frame, RESERVE_TIMEOUT, &[TIMEOUT_SECONDS]);
        put(&mut frame, RESERVE_MODE, &mode.to_be_bytes());
        put(&mut frame, RESERVE_HOST, &self.host.octets());
        put(
            &mut frame,
            RESERVE_TRIGGER_PORT,
            &u32::from(self.trigger_port.number()).to_be_bytes(),
        );
        put(&mut frame, RESERVE_TIME, &self.time.encode());
        put(&mut frame, RESERVE_HOST_ID, &self.host_id.bytes());
        put(&mut frame, RESERVE_OFFSET, &self.offset.0.to_be_bytes());
        frame
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReserveRefusal {
    General,
    PasswordNecessary,
    WrongPassword,
    Occupied,
    UnsupportedVersion,
    HostIdNotRegistered,
    HostIdNotValid,
    SequenceError,
    UsbMode,
    Unrecognized(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReserveReply {
    result: i32,
}

impl ReserveReply {
    /// # Errors
    ///
    /// Refuses a frame that is shorter than a reserve reply or carries another key.
    pub fn decode(frame: &[u8]) -> Result<Self, FrameError> {
        keyed_field(frame, RESERVE_REPLY_LENGTH, FRAME_KEY, RESULT).map(|result| Self {
            result: i32::from_be_bytes(result),
        })
    }

    /// # Errors
    ///
    /// Returns the scanner's reason when it refuses the reserve.
    pub const fn outcome(self) -> Result<(), ReserveRefusal> {
        Err(match self.result {
            ACCEPTED => return Ok(()),
            GENERAL => ReserveRefusal::General,
            PASSWORD_NECESSARY => ReserveRefusal::PasswordNecessary,
            WRONG_PASSWORD => ReserveRefusal::WrongPassword,
            OCCUPIED => ReserveRefusal::Occupied,
            UNSUPPORTED_VERSION => ReserveRefusal::UnsupportedVersion,
            HOST_ID_NOT_REGISTERED => ReserveRefusal::HostIdNotRegistered,
            HOST_ID_NOT_VALID => ReserveRefusal::HostIdNotValid,
            SEQUENCE_ERROR => ReserveRefusal::SequenceError,
            USB_MODE => ReserveRefusal::UsbMode,
            other => ReserveRefusal::Unrecognized(other),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    mac: MacAddress,
}

impl Release {
    #[must_use]
    pub const fn new(mac: MacAddress) -> Self {
        Self { mac }
    }

    #[must_use]
    pub fn encode(self) -> [u8; RELEASE_LENGTH] {
        let mut frame = [0; RELEASE_LENGTH];
        put(&mut frame, LENGTH, &RELEASE_LENGTH_FIELD);
        put(&mut frame, FRAME_KEY, &KEY);
        put(&mut frame, COMMAND, &RELEASE_COMMAND.to_be_bytes());
        put(&mut frame, HOST_MAC, &self.mac.octets());
        frame
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseReply {
    result: i32,
}

impl ReleaseReply {
    /// # Errors
    ///
    /// Refuses a frame that is shorter than a release reply or carries another key.
    pub fn decode(frame: &[u8]) -> Result<Self, FrameError> {
        keyed_field(frame, RELEASE_REPLY_LENGTH, FRAME_KEY, RESULT).map(|result| Self {
            result: i32::from_be_bytes(result),
        })
    }

    /// # Errors
    ///
    /// Returns the scanner's result code when it refuses the release.
    pub const fn outcome(self) -> Result<(), i32> {
        match self.result {
            ACCEPTED => Ok(()),
            refused => Err(refused),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Greeting;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GreetingError {
    Frame(FrameError),
    Refused { status: u32 },
}

impl Greeting {
    /// # Errors
    ///
    /// Refuses a frame that is shorter than a greeting, carries another key, or reports a status other than zero.
    pub fn decode(frame: &[u8]) -> Result<Self, GreetingError> {
        let status = keyed_field(frame, GREETING_LENGTH, FRAME_KEY, GREETING_STATUS)
            .map(u32::from_be_bytes)
            .map_err(GreetingError::Frame)?;
        if status == GREETING_ACCEPTED {
            Ok(Self)
        } else {
            Err(GreetingError::Refused { status })
        }
    }
}
