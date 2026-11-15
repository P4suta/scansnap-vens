use crate::{
    DecodeError, HostId, Ipv4Address, MacAddress, Port,
    wire::{INTERFACE_VERSION, KEY, field, put},
};

const PROBE_LENGTH: usize = 32;
const PROBE_KIND: usize = 4;
const PROBE_HOST: usize = 8;
const PROBE_HOST_MAC: usize = 12;
const PROBE_VERSION: usize = 24;
const SEARCH: u32 = 0;
const KEEPALIVE: u32 = 1;

const REPLY_LENGTH: usize = 144;
const REPLY_KEY: usize = 0;
const REPLY_VALIDITY: usize = 6;
const REPLY_PASSWORD: usize = 12;
const REPLY_ADDRESS: usize = 16;
const REPLY_COMMAND_PORT: usize = 20;
const REPLY_CONTROL_PORT: usize = 24;
const REPLY_MAC: usize = 28;
const REPLY_DEVICE_STATUS: usize = 38;
const REPLY_DEVICE_NAME: usize = 40;
const REPLY_PRODUCT_NAME: usize = 104;
const REPLY_LAST_OWNER: usize = 124;
const REPLY_SERIAL: usize = 132;
const INVALID_REPLY: [u8; 2] = [0xFF, 0xFF];
const NO_PASSWORD: u32 = 0;
const DEVICE_READY: u16 = 0;
const DEVICE_BUSY: u16 = 2;
const TEXT_END: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeKind {
    Search,
    Keepalive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    kind: ProbeKind,
    host: Ipv4Address,
    mac: MacAddress,
}

impl Probe {
    #[must_use]
    pub const fn search(host: Ipv4Address, mac: MacAddress) -> Self {
        Self {
            kind: ProbeKind::Search,
            host,
            mac,
        }
    }

    #[must_use]
    pub const fn keepalive(host: Ipv4Address, mac: MacAddress) -> Self {
        Self {
            kind: ProbeKind::Keepalive,
            host,
            mac,
        }
    }

    #[must_use]
    pub fn encode(self) -> [u8; PROBE_LENGTH] {
        let kind = match self.kind {
            ProbeKind::Search => SEARCH,
            ProbeKind::Keepalive => KEEPALIVE,
        };
        let mut frame = [0; PROBE_LENGTH];
        put(&mut frame, 0, &KEY);
        put(&mut frame, PROBE_KIND, &kind.to_be_bytes());
        put(&mut frame, PROBE_HOST, &self.host.octets());
        put(&mut frame, PROBE_HOST_MAC, &self.mac.octets());
        put(&mut frame, PROBE_VERSION, &INTERFACE_VERSION);
        frame
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Password {
    NotSet,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Busy,
    Unrecognized(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text<const N: usize>([u8; N]);

impl<const N: usize> Text<N> {
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let length = self
            .0
            .iter()
            .position(|byte| *byte == TEXT_END)
            .unwrap_or(N);
        let text = core::str::from_utf8(self.0.get(..length)?).ok()?;
        Some(text.trim_end())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchReply {
    address: Ipv4Address,
    command_port: Port,
    control_port: Port,
    mac: MacAddress,
    password: Password,
    availability: Availability,
    device_name: Text<64>,
    product_name: Text<16>,
    last_owner: HostId,
    serial: Text<12>,
}

fn port(value: [u8; 4]) -> Result<Port, DecodeError> {
    let value = u32::from_be_bytes(value);
    u16::try_from(value)
        .map(Port::new)
        .map_err(|_| DecodeError::PortOutOfRange { value })
}

impl SearchReply {
    /// # Errors
    ///
    /// Refuses a frame that is shorter than a search reply, carries another key, is marked invalid by the scanner, or names a port beyond sixteen bits.
    pub fn decode(frame: &[u8]) -> Result<Self, DecodeError> {
        let truncated = DecodeError::Truncated {
            expected: REPLY_LENGTH,
            actual: frame.len(),
        };
        if frame.len() < REPLY_LENGTH {
            return Err(truncated);
        }
        if field(frame, REPLY_KEY) != Some(KEY) {
            return Err(DecodeError::ForeignKey);
        }
        if field(frame, REPLY_VALIDITY) == Some(INVALID_REPLY) {
            return Err(DecodeError::NotAReply);
        }
        let password = match field(frame, REPLY_PASSWORD).map(u32::from_be_bytes) {
            Some(NO_PASSWORD) => Password::NotSet,
            Some(_) => Password::Required,
            None => return Err(truncated),
        };
        let availability = match field(frame, REPLY_DEVICE_STATUS).map(u16::from_be_bytes) {
            Some(DEVICE_READY) => Availability::Ready,
            Some(DEVICE_BUSY) => Availability::Busy,
            Some(status) => Availability::Unrecognized(status),
            None => return Err(truncated),
        };
        Ok(Self {
            address: Ipv4Address::new(field(frame, REPLY_ADDRESS).ok_or(truncated)?),
            command_port: port(field(frame, REPLY_COMMAND_PORT).ok_or(truncated)?)?,
            control_port: port(field(frame, REPLY_CONTROL_PORT).ok_or(truncated)?)?,
            mac: MacAddress::new(field(frame, REPLY_MAC).ok_or(truncated)?),
            password,
            availability,
            device_name: Text(field(frame, REPLY_DEVICE_NAME).ok_or(truncated)?),
            product_name: Text(field(frame, REPLY_PRODUCT_NAME).ok_or(truncated)?),
            last_owner: HostId::new(field(frame, REPLY_LAST_OWNER).ok_or(truncated)?),
            serial: Text(field(frame, REPLY_SERIAL).ok_or(truncated)?),
        })
    }

    #[must_use]
    pub const fn address(&self) -> Ipv4Address {
        self.address
    }

    #[must_use]
    pub const fn command_port(&self) -> Port {
        self.command_port
    }

    #[must_use]
    pub const fn control_port(&self) -> Port {
        self.control_port
    }

    #[must_use]
    pub const fn mac(&self) -> MacAddress {
        self.mac
    }

    #[must_use]
    pub const fn password(&self) -> Password {
        self.password
    }

    #[must_use]
    pub const fn availability(&self) -> Availability {
        self.availability
    }

    #[must_use]
    pub const fn last_owner(&self) -> HostId {
        self.last_owner
    }

    #[must_use]
    pub const fn device_name(&self) -> &Text<64> {
        &self.device_name
    }

    #[must_use]
    pub const fn product_name(&self) -> &Text<16> {
        &self.product_name
    }

    #[must_use]
    pub const fn serial(&self) -> &Text<12> {
        &self.serial
    }
}
