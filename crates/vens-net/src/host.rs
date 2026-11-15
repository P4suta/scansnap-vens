use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket},
    time::Duration,
};

use crate::{channel::Stream, search::Datagram};

const ANY_PORT: u16 = 0;
const DISCARD_PORT: u16 = 9;

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
