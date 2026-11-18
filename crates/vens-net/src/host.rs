use std::{
    fmt,
    fs::OpenOptions,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket},
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use vens_core::{
    Ipv4Address, Port,
    control::LocalTime,
    job::Pause,
    jpeg::{self, JpegError},
    keepalive::Elapsed,
    scan::Side,
};

use crate::{
    channel::Stream,
    network::{Connection, Network},
    scan::{Clock, PageSink},
    search::Datagram,
};

const ANY_PORT: u16 = 0;
const DISCARD_PORT: u16 = 9;
const RETRY_INTERVAL: Duration = Duration::from_millis(100);
const ROUTE_PROBE: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
const UDP_RECEIVE_ATTEMPTS: usize = 16;

/// Sleeps through each pause and reads the time elapsed since its creation from the monotonic clock.
#[derive(Debug)]
pub struct SleepingClock {
    start: Instant,
}

impl SleepingClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SleepingClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SleepingClock {
    fn wait(&mut self, pause: Pause) {
        match pause {
            Pause::NoDataYet | Pause::Busy => thread::sleep(RETRY_INTERVAL),
        }
    }

    fn now(&mut self) -> Elapsed {
        Elapsed::from_millis(u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX))
    }
}

#[derive(Debug)]
pub enum SinkError {
    Io { path: PathBuf, error: io::Error },
    Image(JpegError),
}

impl fmt::Display for SinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, error } => write!(formatter, "{}: {error}", path.display()),
            Self::Image(error) => write!(formatter, "the image header is malformed ({error:?})"),
        }
    }
}

#[derive(Debug)]
pub struct DirectorySink {
    directory: PathBuf,
    image: Vec<u8>,
    written: Vec<PathBuf>,
}

impl DirectorySink {
    #[must_use]
    pub const fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            image: Vec::new(),
            written: Vec::new(),
        }
    }

    #[must_use]
    pub fn written(&self) -> &[PathBuf] {
        &self.written
    }
}

impl PageSink for DirectorySink {
    type Error = SinkError;

    fn append(&mut self, _: u32, _: Side, data: &[u8]) -> Result<(), SinkError> {
        self.image.extend_from_slice(data);
        Ok(())
    }

    fn finish(&mut self, sheet: u32, side: Side, height: u16) -> Result<(), SinkError> {
        let mut image = std::mem::take(&mut self.image);
        jpeg::set_height(&mut image, height).map_err(SinkError::Image)?;
        let side = match side {
            Side::Front => "front",
            Side::Back => "back",
        };
        let number = sheet.saturating_add(1);
        let path = self.directory.join(format!("sheet-{number:03}-{side}.jpg"));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| file.write_all(&image));
        if let Err(error) = written {
            return Err(SinkError::Io { path, error });
        }
        self.written.push(path);
        Ok(())
    }
}

#[derive(Debug)]
pub struct UdpPort {
    socket: UdpSocket,
    scanner: Option<Ipv4Addr>,
}

impl UdpPort {
    /// # Errors
    ///
    /// Fails when the socket can't be bound or configured.
    pub fn open(deadline: Duration) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ANY_PORT))?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(deadline))?;
        Ok(Self {
            socket,
            scanner: None,
        })
    }

    /// Turns the port into one whose receive returns at once, for keepalive ticks between commands.
    ///
    /// # Errors
    ///
    /// Fails when the socket can't change its mode.
    pub fn into_nonblocking(self) -> io::Result<Self> {
        self.socket.set_nonblocking(true)?;
        Ok(self)
    }
}

impl Datagram for UdpPort {
    type Error = io::Error;

    fn send(&mut self, frame: &[u8], target: SocketAddrV4) -> io::Result<()> {
        self.socket.send_to(frame, target).map(|_| ())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> io::Result<Option<usize>> {
        for _ in 0..UDP_RECEIVE_ATTEMPTS {
            match self.socket.recv_from(buffer) {
                Ok((length, peer))
                    if self
                        .scanner
                        .is_none_or(|scanner| peer.ip() == IpAddr::V4(scanner)) =>
                {
                    return Ok(Some(length));
                }
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
}

#[derive(Debug)]
pub struct TcpPort(TcpStream);

impl TcpPort {
    /// # Errors
    ///
    /// Fails when the peer doesn't accept within the deadline or the socket can't be configured.
    pub fn connect(address: SocketAddrV4, deadline: Duration) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&SocketAddr::V4(address), deadline)?;
        stream.set_read_timeout(Some(deadline))?;
        stream.set_write_timeout(Some(deadline))?;
        stream.set_nodelay(true)?;
        Ok(Self(stream))
    }
}

impl Stream for TcpPort {
    type Error = io::Error;

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)
    }

    fn receive(&mut self, buffer: &mut [u8]) -> io::Result<()> {
        self.0.read_exact(buffer)
    }
}

/// Names the local address that the routing table selects for `target`, without sending a packet.
///
/// # Errors
///
/// Fails when no route to `target` exists or the selected address isn't IPv4.
pub fn local_address_toward(target: Ipv4Addr) -> io::Result<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ANY_PORT))?;
    socket.set_broadcast(true)?;
    socket.connect((target, DISCARD_PORT))?;
    match socket.local_addr()? {
        SocketAddr::V4(address) => Ok(*address.ip()),
        SocketAddr::V6(_) => Err(io::Error::other("the selected local address isn't IPv4")),
    }
}

/// Reaches the scanner through the sockets, the routing table, and the clocks of this host.
#[derive(Debug, Clone, Copy)]
pub struct SystemNetwork {
    probe_interval: Duration,
    deadline: Duration,
}

impl SystemNetwork {
    /// Waits `probe_interval` for each discovery reply and `deadline` for each connection and transfer.
    #[must_use]
    pub const fn new(probe_interval: Duration, deadline: Duration) -> Self {
        Self {
            probe_interval,
            deadline,
        }
    }
}

impl Network for SystemNetwork {
    type Error = io::Error;
    type Discovery = UdpPort;
    type Keepalive = UdpPort;
    type Stream = TcpPort;
    type Trigger = UdpSocket;
    type Clock = SleepingClock;

    fn local_address(&mut self, target: Ipv4Addr) -> io::Result<Ipv4Address> {
        let route = if target.is_broadcast() {
            ROUTE_PROBE
        } else {
            target
        };
        local_address_toward(route).map(|address| Ipv4Address::new(address.octets()))
    }

    fn open_discovery(&mut self) -> io::Result<UdpPort> {
        UdpPort::open(self.probe_interval)
    }

    fn keepalive(&mut self, discovery: UdpPort, scanner: Ipv4Addr) -> io::Result<UdpPort> {
        let mut port = discovery.into_nonblocking()?;
        port.scanner = Some(scanner);
        Ok(port)
    }

    fn open_trigger(&mut self, local: Ipv4Address) -> io::Result<(UdpSocket, Port)> {
        let socket = UdpSocket::bind((Ipv4Addr::from(local.octets()), ANY_PORT))?;
        socket.set_nonblocking(true)?;
        match socket.local_addr()? {
            SocketAddr::V4(address) => {
                let port = Port::new(address.port());
                Ok((socket, port))
            }
            SocketAddr::V6(_) => Err(io::Error::other("the trigger socket isn't IPv4")),
        }
    }

    fn connect(&mut self, endpoint: SocketAddrV4, _: Connection) -> io::Result<TcpPort> {
        TcpPort::connect(endpoint, self.deadline)
    }

    fn local_time(&mut self) -> io::Result<LocalTime> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| LocalTime::from_unix_seconds(elapsed.as_secs()))
            .ok_or_else(|| io::Error::other("the system clock is outside the supported range"))
    }

    fn clock(&mut self) -> SleepingClock {
        SleepingClock::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Datagram, Duration, Ipv4Addr, Network, SystemNetwork, UdpSocket, io};

    fn receive_keepalive(scanner: Ipv4Addr) -> io::Result<Option<usize>> {
        let timeout = Duration::from_secs(1);
        let mut network = SystemNetwork::new(timeout, timeout);
        let discovery = network.open_discovery()?;
        let destination = (Ipv4Addr::LOCALHOST, discovery.socket.local_addr()?.port());
        let mut port = network.keepalive(discovery, scanner)?;
        port.socket.set_nonblocking(false)?;
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        sender.send_to(b"reply", destination)?;
        port.receive(&mut [0; 16])
    }

    #[test]
    fn the_keepalive_port_only_accepts_the_selected_scanner() {
        assert_eq!(receive_keepalive(Ipv4Addr::LOCALHOST).ok(), Some(Some(5)));
        assert_eq!(
            receive_keepalive(Ipv4Addr::new(192, 0, 2, 7)).ok(),
            Some(None)
        );
    }
}
