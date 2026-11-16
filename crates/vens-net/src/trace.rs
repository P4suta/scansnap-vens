use std::fmt;

const FIELDS: usize = 3;
const HEX_RADIX: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Discovery,
    Command,
    Control,
}

impl Channel {
    const fn name(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::Command => "command",
            Self::Control => "control",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "discovery" => Some(Self::Discovery),
            "command" => Some(Self::Command),
            "control" => Some(Self::Control),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

impl Direction {
    const fn symbol(self) -> &'static str {
        match self {
            Self::Sent => ">",
            Self::Received => "<",
        }
    }

    fn from_symbol(symbol: &str) -> Option<Self> {
        match symbol {
            ">" => Some(Self::Sent),
            "<" => Some(Self::Received),
            _ => None,
        }
    }
}

/// One transfer of at least one byte, seen from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub channel: Channel,
    pub direction: Direction,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    UnknownChannel,
    UnknownDirection,
    MissingBytes,
    NotHex,
    OddLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceError {
    pub line: usize,
    pub fault: Fault,
}

struct Hex<'bytes>(&'bytes [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0
            .iter()
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

impl fmt::Display for Event {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} {}",
            self.channel.name(),
            self.direction.symbol(),
            Hex(&self.bytes)
        )
    }
}

/// Writes one line for each event.
#[must_use]
pub fn render(events: &[Event]) -> String {
    let mut text = String::new();
    for event in events {
        text.push_str(&event.to_string());
        text.push('\n');
    }
    text
}

fn decode_hex(text: &str) -> Result<Vec<u8>, Fault> {
    let digits = text.as_bytes();
    if digits.is_empty() {
        return Err(Fault::MissingBytes);
    }
    if !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err(Fault::NotHex);
    }
    let (pairs, remainder) = digits.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(Fault::OddLength);
    }
    pairs
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair.as_slice())
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, HEX_RADIX).ok())
                .ok_or(Fault::NotHex)
        })
        .collect()
}

fn parse_line(line: &str) -> Result<Event, Fault> {
    let mut fields = line.splitn(FIELDS, ' ');
    let channel = fields
        .next()
        .and_then(Channel::from_name)
        .ok_or(Fault::UnknownChannel)?;
    let direction = fields
        .next()
        .and_then(Direction::from_symbol)
        .ok_or(Fault::UnknownDirection)?;
    let bytes = decode_hex(fields.next().unwrap_or_default())?;
    Ok(Event {
        channel,
        direction,
        bytes,
    })
}

/// # Errors
///
/// Refuses the first line that names an unknown channel or direction, or that carries no bytes or anything besides hex digit pairs.
pub fn parse(text: &str) -> Result<Vec<Event>, TraceError> {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            parse_line(line).map_err(|fault| TraceError {
                line: index.saturating_add(1),
                fault,
            })
        })
        .collect()
}
