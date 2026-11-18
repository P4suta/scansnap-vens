use core::num::NonZeroU8;

use crate::wire::{field, keyed_field};

/// The datagram size in the iX1300 protocol, section 6.
pub const PACKET_LENGTH: usize = 48;
const KEY_OFFSET: usize = 4;
const CODE_OFFSET: usize = 8;
const SEQUENCE_OFFSET: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonEvent {
    Press(NonZeroU8),
    PowerOff,
}

impl ButtonEvent {
    /// Recognizes the button events that need an action, as described in section 6.
    #[must_use]
    pub fn decode(packet: &[u8]) -> Option<Self> {
        if packet.len() != PACKET_LENGTH {
            return None;
        }
        let code = keyed_field(packet, PACKET_LENGTH, KEY_OFFSET, CODE_OFFSET).ok()?;
        let [sequence] = field(packet, SEQUENCE_OFFSET)?;
        let sequence = NonZeroU8::new(sequence)?;
        match u32::from_be_bytes(code) {
            1 => Some(Self::Press(sequence)),
            0x10 => Some(Self::PowerOff),
            _ => None,
        }
    }
}

/// Remembers the last 16 presses even when a repeat arrives during a later job.
#[derive(Debug, Default)]
pub struct RecentPresses([u8; 16]);

impl RecentPresses {
    /// Returns true only for the first copy of a recent notification.
    pub fn accept(&mut self, sequence: NonZeroU8) -> bool {
        let value = sequence.get();
        if self.0.contains(&value) {
            return false;
        }
        self.0.rotate_left(1);
        if let Some(last) = self.0.last_mut() {
            *last = value;
        }
        true
    }
}
