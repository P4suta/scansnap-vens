use crate::{HostId, wire::field};

const LENGTH: usize = 0x30;
const FEED: usize = 2;
const CHUTE: usize = 3;
const HOST_ID: usize = 0x20;
const FEED_SENSOR: u8 = 0x80;
const CHUTE_EMPTY: u8 = 0x80;
const EXIT_SENSOR: u8 = 0x01;
const NO_HOST: [u8; 8] = [0xFF; 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperPath {
    TopChute,
    FrontSlot,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusError {
    Truncated { expected: usize, actual: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwareStatus {
    feed: u8,
    chute: u8,
    host_id: [u8; 8],
}

impl HardwareStatus {
    /// # Errors
    ///
    /// Refuses a reply shorter than the form that carries the registered host identifier.
    pub fn decode(reply: &[u8]) -> Result<Self, StatusError> {
        let truncated = StatusError::Truncated {
            expected: LENGTH,
            actual: reply.len(),
        };
        if reply.len() < LENGTH {
            return Err(truncated);
        }
        let [feed] = field(reply, FEED).ok_or(truncated)?;
        let [chute] = field(reply, CHUTE).ok_or(truncated)?;
        Ok(Self {
            feed,
            chute,
            host_id: field(reply, HOST_ID).ok_or(truncated)?,
        })
    }

    #[must_use]
    pub const fn paper(&self) -> PaperPath {
        if self.chute & CHUTE_EMPTY == 0 {
            PaperPath::TopChute
        } else if self.feed & FEED_SENSOR == 0 {
            PaperPath::Absent
        } else {
            PaperPath::FrontSlot
        }
    }

    #[must_use]
    pub const fn exit_sensor(&self) -> bool {
        self.chute & EXIT_SENSOR != 0
    }

    #[must_use]
    pub fn registered_host(&self) -> Option<HostId> {
        (self.host_id != NO_HOST).then_some(HostId::new(self.host_id))
    }
}
