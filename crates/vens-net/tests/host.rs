//! Loopback contract tests for the socket adapters.

use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use vens_core::Ipv4Address;
use vens_net::{
    channel::Stream,
    host::{SystemNetwork, TcpPort, UdpPort, local_address_toward},
    network::{Connection, Network},
    search::Datagram,
};

const SHORT: Duration = Duration::from_millis(50);
const GENEROUS: Duration = Duration::from_secs(5);

fn loopback(address: SocketAddr) -> io::Result<SocketAddrV4> {
    match address {
        SocketAddr::V4(address) => Ok(address),
        SocketAddr::V6(_) => Err(io::Error::other("loopback test needs an IPv4 address")),
    }
}

fn datagram_exchange() -> io::Result<(Vec<u8>, Option<usize>, Option<usize>)> {
    let peer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
    peer.set_read_timeout(Some(GENEROUS))?;
    let target = loopback(peer.local_addr()?)?;
    let mut port = UdpPort::open(SHORT)?;
    port.send(b"probe", target)?;
    let mut seen = [0; 16];
    let (length, sender) = peer.recv_from(&mut seen)?;
    peer.send_to(b"reply", sender)?;
    let mut buffer = [0; 16];
    let delivered = port.receive(&mut buffer)?;
    let silence = port.receive(&mut buffer)?;
    Ok((
        seen.get(..length).unwrap_or_default().to_vec(),
        delivered,
        silence,
    ))
}

fn serve(listener: &TcpListener) -> io::Result<Vec<u8>> {
    let (mut stream, _) = listener.accept()?;
    let mut request = [0; 5];
    stream.read_exact(&mut request)?;
    stream.write_all(b"reply")?;
    Ok(request.to_vec())
}

fn stream_exchange() -> io::Result<(Vec<u8>, [u8; 5])> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let address = loopback(listener.local_addr()?)?;
    let (served, requests) = mpsc::channel();
    thread::scope(|scope| {
        scope.spawn(|| served.send(serve(&listener)));
        let mut port = TcpPort::connect(address, GENEROUS)?;
        port.send(b"hello")?;
        let mut reply = [0; 5];
        port.receive(&mut reply)?;
        let request = requests
            .recv_timeout(GENEROUS)
            .map_err(|_| io::Error::other("the server thread sent no result"))??;
        Ok((request, reply))
    })
}

fn silent_stream() -> io::Result<io::ErrorKind> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let address = loopback(listener.local_addr()?)?;
    let mut port = TcpPort::connect(address, SHORT)?;
    let (_held_open, _) = listener.accept()?;
    let mut buffer = [0; 1];
    match port.receive(&mut buffer) {
        Ok(()) => Err(io::Error::other("a silent peer delivered a byte")),
        Err(error) => Ok(error.kind()),
    }
}

fn nonblocking_silence() -> io::Result<(Option<usize>, bool)> {
    let mut port = UdpPort::open(Duration::from_secs(60))?.into_nonblocking()?;
    let mut buffer = [0; 16];
    let started = Instant::now();
    let silence = port.receive(&mut buffer)?;
    Ok((silence, started.elapsed() < GENEROUS))
}

type SystemRound = (Ipv4Address, bool, bool, Option<usize>, bool, bool);

fn system_network_round() -> io::Result<SystemRound> {
    let mut network = SystemNetwork::new(SHORT, GENEROUS);
    let local = network.local_address(Ipv4Addr::LOCALHOST)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let endpoint = loopback(listener.local_addr()?)?;
    let stream = network.connect(endpoint, Connection::Command);
    let (_trigger, port) = network.open_trigger(local)?;
    let time = network.local_time();
    let discovery = network.open_discovery()?;
    let mut keepalive = network.keepalive(discovery, Ipv4Addr::LOCALHOST)?;
    let mut buffer = [0; 8];
    let started = Instant::now();
    let silence = keepalive.receive(&mut buffer)?;
    Ok((
        local,
        stream.is_ok(),
        port.number() != 0,
        silence,
        started.elapsed() < GENEROUS,
        time.is_ok(),
    ))
}

#[test]
fn the_system_network_opens_real_ports_on_loopback() {
    assert_eq!(
        system_network_round().ok(),
        Some((
            Ipv4Address::new([127, 0, 0, 1]),
            true,
            true,
            None,
            true,
            true
        ))
    );
}

#[test]
fn a_nonblocking_datagram_port_reports_silence_at_once() {
    assert_eq!(nonblocking_silence().ok(), Some((None, true)));
}

#[test]
fn a_datagram_port_delivers_frames_and_reports_silence_as_none() {
    assert_eq!(
        datagram_exchange().ok(),
        Some((b"probe".to_vec(), Some(5), None))
    );
}

#[test]
fn a_stream_port_sends_and_receives_exact_byte_counts() {
    assert_eq!(stream_exchange().ok(), Some((b"hello".to_vec(), *b"reply")));
}

#[test]
fn a_stream_port_gives_up_at_its_deadline() {
    let kind = silent_stream().ok();
    assert!(matches!(
        kind,
        Some(io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
    ));
}

#[test]
fn the_local_address_toward_loopback_is_loopback() {
    assert_eq!(
        local_address_toward(Ipv4Addr::LOCALHOST).ok(),
        Some(Ipv4Addr::LOCALHOST)
    );
}
