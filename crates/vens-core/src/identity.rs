#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv4Address([u8; 4]);

impl Ipv4Address {
    #[must_use]
    pub const fn new(octets: [u8; 4]) -> Self {
        Self(octets)
    }

    #[must_use]
    pub const fn octets(self) -> [u8; 4] {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    #[must_use]
    pub const fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    #[must_use]
    pub const fn octets(self) -> [u8; 6] {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostId([u8; 8]);

impl HostId {
    #[must_use]
    pub const fn new(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; 8] {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Port(u16);

impl Port {
    #[must_use]
    pub const fn new(number: u16) -> Self {
        Self(number)
    }

    #[must_use]
    pub const fn number(self) -> u16 {
        self.0
    }
}
