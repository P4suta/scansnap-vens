use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddrV4},
    num::NonZeroU8,
};

use crate::{
    channel::{ChannelError, CommandChannel},
    control::{ControlChannel, ControlError},
    keepalive::DatagramKeepalive,
    network::{Connection, Network},
    scan::{Clock, PageSink},
    search::{SearchError, search},
    session::{Host, SessionError, scan_once},
};
use vens_core::{
    Ipv4Address, MacAddress,
    command::ScsiStatus,
    discovery::{Probe, SearchReply},
    scan::Command,
    status::{HardwareStatus, StatusError},
};

/// Identifies this host in every frame.
/// The value is locally administered and fixed.
pub const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);

const DISCOVERY_PORT: u16 = 52217;
/// Gives a scanner in power save about ten seconds of half-second probe windows to wake up.
const SEARCH_ATTEMPTS: NonZeroU8 = NonZeroU8::MIN.saturating_add(19);
const STATUS_CAPACITY: usize = 64;
const REPLY_CAPACITY: usize = 0x0030_0040;

#[derive(Debug, PartialEq, Eq)]
pub enum Failure<E, K> {
    /// The host has no route toward the search target.
    Route {
        target: Ipv4Addr,
        error: E,
    },
    Discovery(E),
    NoReply {
        target: Ipv4Addr,
        attempts: u8,
    },
    Trigger(E),
    Clock(E),
    Connect {
        connection: Connection,
        error: E,
    },
    Keepalive(E),
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

/// The outcome of a scan and the first failure of its keepalive port, which never stops the job.
#[derive(Debug, PartialEq, Eq)]
pub struct ScanRun<E, K> {
    pub outcome: Result<Scanned, Failure<E, K>>,
    pub keepalive_failure: Option<E>,
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
    let local = network
        .local_address(target)
        .map_err(|error| Failure::Route { target, error })?;
    let mut discovery = network.open_discovery().map_err(Failure::Discovery)?;
    let reply = search(
        &mut discovery,
        Probe::search(local, HOST_MAC),
        SocketAddrV4::new(target, DISCOVERY_PORT),
        SEARCH_ATTEMPTS,
    )
    .map_err(|error| match error {
        SearchError::Port(error) => Failure::Discovery(error),
        SearchError::NoReply { attempts } => Failure::NoReply { target, attempts },
    })?;
    Ok(Found {
        reply,
        local,
        discovery,
    })
}

fn scanner(reply: &SearchReply, port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::from(reply.address().octets()), port)
}

fn connect<N: Network, K>(
    network: &mut N,
    endpoint: SocketAddrV4,
    connection: Connection,
) -> Result<N::Stream, Failure<N::Error, K>> {
    network
        .connect(endpoint, connection)
        .map_err(|error| Failure::Connect { connection, error })
}

fn open_command<N: Network, K>(
    network: &mut N,
    reply: &SearchReply,
    local: Ipv4Address,
) -> Result<CommandChannel<N::Stream>, Failure<N::Error, K>> {
    let endpoint = scanner(reply, reply.command_port().number());
    let stream = connect(network, endpoint, Connection::Command)?;
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
/// The run carries the outcome and any failure of the keepalive port, which never stops the job.
pub fn scan<N: Network, K: PageSink>(
    network: &mut N,
    target: Ipv4Addr,
    sink: &mut K,
) -> ScanRun<N::Error, K::Error> {
    let mut keepalive_failure = None;
    let outcome = scan_with(network, target, sink, &mut keepalive_failure);
    ScanRun {
        outcome,
        keepalive_failure,
    }
}

fn scan_with<N: Network, K: PageSink>(
    network: &mut N,
    target: Ipv4Addr,
    sink: &mut K,
    keepalive_failure: &mut Option<N::Error>,
) -> Result<Scanned, Failure<N::Error, K::Error>> {
    let found = find(network, target)?;
    let (_trigger, trigger_port) = network
        .open_trigger(found.local)
        .map_err(Failure::Trigger)?;
    let time = network.local_time().map_err(Failure::Clock)?;
    let mut command = open_command(network, &found.reply, found.local)?;
    let control_endpoint = scanner(&found.reply, found.reply.control_port().number());
    let control_stream = connect(network, control_endpoint, Connection::Control)?;
    let mut control = ControlChannel::open(control_stream).map_err(Failure::Control)?;
    let mut clock = network.clock();
    let keepalive_port = network
        .keepalive(found.discovery)
        .map_err(Failure::Keepalive)?;
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
    let (_, failure) = keepalive.into_parts();
    *keepalive_failure = failure;
    outcome
        .map(|summary| Scanned {
            sheets: summary.sheets,
            keepalive_replies,
        })
        .map_err(Failure::Session)
}
