use std::{
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket},
    path::PathBuf,
    thread,
    time::Duration,
};

use vens_core::{
    job::Pause,
    jpeg::{self, JpegError},
    scan::Side,
};

use crate::{
    channel::Stream,
    scan::{Clock, PageSink},
    search::Datagram,
};

const ANY_PORT: u16 = 0;
const DISCARD_PORT: u16 = 9;
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Default)]
pub struct SleepingClock;

impl Clock for SleepingClock {
    fn wait(&mut self, pause: Pause) {
        match pause {
            Pause::NoDataYet | Pause::Busy => thread::sleep(RETRY_INTERVAL),
        }
    }
}

#[derive(Debug)]
pub enum SinkError {
    Io(io::Error),
    Image(JpegError),
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
        fs::write(&path, &image).map_err(SinkError::Io)?;
        self.written.push(path);
        Ok(())
    }
}

#[derive(Debug)]
pub struct UdpPort(UdpSocket);

impl UdpPort {
    /// # Errors
    ///
    /// Fails when the socket can't be bound or configured.
    pub fn open(deadline: Duration) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ANY_PORT))?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(deadline))?;
        Ok(Self(socket))
    }
}

impl Datagram for UdpPort {
    type Error = io::Error;

    fn send(&mut self, frame: &[u8], target: SocketAddrV4) -> io::Result<()> {
        self.0.send_to(frame, target).map(|_| ())
    }

    fn receive(&mut self, buffer: &mut [u8]) -> io::Result<Option<usize>> {
        match self.0.recv_from(buffer) {
            Ok((length, _)) => Ok(Some(length)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
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
    socket.connect((target, DISCARD_PORT))?;
    match socket.local_addr()? {
        SocketAddr::V4(address) => Ok(*address.ip()),
        SocketAddr::V6(_) => Err(io::Error::other("the selected local address isn't IPv4")),
    }
}
