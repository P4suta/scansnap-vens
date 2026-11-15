use vens_core::{
    Ipv4Address, MacAddress,
    command::{ReplyHeader, ReplyHeaderError, ScsiStatus},
    control::{Greeting, GreetingError},
    scan::Command,
};

const GREETING_LENGTH: usize = 16;
const REPLY_HEADER_LENGTH: usize = 40;

pub trait Stream {
    type Error;

    /// # Errors
    ///
    /// Reports the port's own failure to send every byte.
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Fills the whole buffer.
    ///
    /// # Errors
    ///
    /// Reports the port's own failure to receive every byte.
    fn receive(&mut self, buffer: &mut [u8]) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChannelError<E> {
    Port(E),
    Greeting(GreetingError),
    Header(ReplyHeaderError),
    ReplyTooLarge { declared: u32, capacity: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Reply<'buffer> {
    pub status: ScsiStatus,
    pub data: &'buffer [u8],
}

#[derive(Debug)]
pub struct CommandChannel<S> {
    stream: S,
    mac: MacAddress,
    host: Ipv4Address,
}

impl<S: Stream> CommandChannel<S> {
    /// # Errors
    ///
    /// Fails when the stream fails or the scanner's greeting is not the accepted one.
    pub fn open(
        mut stream: S,
        mac: MacAddress,
        host: Ipv4Address,
    ) -> Result<Self, ChannelError<S::Error>> {
        let mut greeting = [0; GREETING_LENGTH];
        stream.receive(&mut greeting).map_err(ChannelError::Port)?;
        Greeting::decode(&greeting).map_err(ChannelError::Greeting)?;
        Ok(Self { stream, mac, host })
    }

    /// # Errors
    ///
    /// Fails when the stream fails, the reply header is refused, or the reply data exceeds the buffer.
    pub fn execute<'buffer>(
        &mut self,
        command: &Command,
        buffer: &'buffer mut [u8],
    ) -> Result<Reply<'buffer>, ChannelError<S::Error>> {
        let request = command.request(self.mac, self.host).encode();
        self.stream.send(&request).map_err(ChannelError::Port)?;
        let payload = command.payload();
        if !payload.is_empty() {
            self.stream.send(payload).map_err(ChannelError::Port)?;
        }
        let mut header = [0; REPLY_HEADER_LENGTH];
        self.stream
            .receive(&mut header)
            .map_err(ChannelError::Port)?;
        let header = ReplyHeader::decode(&header).map_err(ChannelError::Header)?;
        let declared = header.payload_length();
        let capacity = buffer.len();
        let data = usize::try_from(declared)
            .ok()
            .and_then(|length| buffer.get_mut(..length))
            .ok_or(ChannelError::ReplyTooLarge { declared, capacity })?;
        self.stream.receive(data).map_err(ChannelError::Port)?;
        Ok(Reply {
            status: header.status(),
            data,
        })
    }

    #[must_use]
    pub fn into_stream(self) -> S {
        self.stream
    }
}
