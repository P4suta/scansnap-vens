use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddrV4},
};

use vens_core::{
    Ipv4Address, MacAddress,
    command::ScsiStatus,
    discovery::{Probe, SearchReply},
    scan::Command,
    status::{HardwareStatus, StatusError},
};
use vens_net::{
    channel::{ChannelError, CommandChannel},
    control::{ControlChannel, ControlError},
    keepalive::DatagramKeepalive,
    network::{Connection, Network},
    scan::{Clock, PageSink},
    search::{SearchError, search},
    session::{Host, SessionError, scan_once},
};

/// Identifies this host in every frame.
/// The value is locally administered and fixed.
pub const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);

const DISCOVERY_PORT: u16 = 52217;
const STATUS_CAPACITY: usize = 64;
const REPLY_CAPACITY: usize = 0x0030_0040;

#[derive(Debug, PartialEq, Eq)]
pub enum Failure<E, K> {
    Network(E),
    Search(SearchError<E>),
    Command(ChannelError<E>),
    Control(ControlError<E>),
    StatusRefused(ScsiStatus),
    Status(StatusError),
    Session(SessionError<E, K>),
}

/// The failure of a command that writes no images.
pub type QueryFailure<E> = Failure<E, Infallible>;

/// The scanner state that `status` reads without a reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    pub reply: SearchReply,
    pub status: HardwareStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scanned {
    pub sheets: u32,
    pub keepalive_replies: u32,
}

struct Found<D> {
    reply: SearchReply,
    local: Ipv4Address,
    discovery: D,
}

fn find<N: Network, K>(
    network: &mut N,
    target: Ipv4Addr,
) -> Result<Found<N::Discovery>, Failure<N::Error, K>> {
    let local = network.local_address(target).map_err(Failure::Network)?;
    let mut discovery = network.open_discovery().map_err(Failure::Network)?;
    let reply = search(
        &mut discovery,
        Probe::search(local, HOST_MAC),
        SocketAddrV4::new(target, DISCOVERY_PORT),
    )
    .map_err(Failure::Search)?;
    Ok(Found {
        reply,
        local,
        discovery,
    })
}

fn scanner(reply: &SearchReply, port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::from(reply.address().octets()), port)
}

fn open_command<N: Network, K>(
    network: &mut N,
    reply: &SearchReply,
    local: Ipv4Address,
) -> Result<CommandChannel<N::Stream>, Failure<N::Error, K>> {
    let endpoint = scanner(reply, reply.command_port().number());
    let stream = network
        .connect(endpoint, Connection::Command)
        .map_err(Failure::Network)?;
    CommandChannel::open(stream, HOST_MAC, local).map_err(Failure::Command)
}

/// Finds the scanner at `target`, which may be the broadcast address.
///
/// # Errors
///
/// Fails when the network fails or no scanner answers.
pub fn discover<N: Network>(
    network: &mut N,
    target: Ipv4Addr,
) -> Result<SearchReply, QueryFailure<N::Error>> {
    find(network, target).map(|found| found.reply)
}

/// Finds the scanner and reads its paper sensors and registered host without reserving it.
///
/// # Errors
///
/// Fails when the network fails, no scanner answers, or the status reply is refused or malformed.
pub fn status<N: Network>(
    network: &mut N,
    target: Ipv4Addr,
) -> Result<State, QueryFailure<N::Error>> {
    let found = find(network, target)?;
    let mut command = open_command(network, &found.reply, found.local)?;
    let mut buffer = [0; STATUS_CAPACITY];
    let answer = command
        .execute(&Command::hardware_status(), &mut buffer)
        .map_err(Failure::Command)?;
    if answer.status != ScsiStatus::Good {
        return Err(Failure::StatusRefused(answer.status));
    }
    let status = HardwareStatus::decode(answer.data).map_err(Failure::Status)?;
    Ok(State {
        reply: found.reply,
        status,
    })
}

/// Finds the scanner, scans the stack in its top chute into the sink, and keeps the reserve alive meanwhile.
///
/// # Errors
///
/// Fails when the network fails, no scanner answers, a connection fails, or the session fails.
pub fn scan<N: Network, K: PageSink>(
    network: &mut N,
    target: Ipv4Addr,
    sink: &mut K,
) -> Result<Scanned, Failure<N::Error, K::Error>> {
    let found = find(network, target)?;
    let (_trigger, trigger_port) = network
        .open_trigger(found.local)
        .map_err(Failure::Network)?;
    let time = network.local_time().map_err(Failure::Network)?;
    let mut command = open_command(network, &found.reply, found.local)?;
    let control_endpoint = scanner(&found.reply, found.reply.control_port().number());
    let control_stream = network
        .connect(control_endpoint, Connection::Control)
        .map_err(Failure::Network)?;
    let mut control = ControlChannel::open(control_stream).map_err(Failure::Control)?;
    let mut clock = network.clock();
    let keepalive_port = network
        .keepalive(found.discovery)
        .map_err(Failure::Network)?;
    let mut keepalive = DatagramKeepalive::new(
        keepalive_port,
        Probe::keepalive(found.local, HOST_MAC),
        scanner(&found.reply, DISCOVERY_PORT),
        clock.now(),
    );
    let host = Host {
        mac: HOST_MAC,
        address: found.local,
        trigger_port,
        time,
    };
    let mut buffer = vec![0_u8; REPLY_CAPACITY];
    let outcome = scan_once(
        &mut command,
        &mut control,
        &mut keepalive,
        host,
        &mut clock,
        sink,
        &mut buffer,
    );
    let keepalive_replies = keepalive.replies();
    outcome
        .map(|summary| Scanned {
            sheets: summary.sheets,
            keepalive_replies,
        })
        .map_err(Failure::Session)
}
