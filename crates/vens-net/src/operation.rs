use std::{
    convert::Infallible,
    net::{Ipv4Addr, SocketAddrV4},
    num::NonZeroU8,
};

use vens_core::{
    Ipv4Address, MacAddress,
    command::ScsiStatus,
    discovery::{Probe, SearchReply},
    scan::Command,
    status::{HardwareStatus, StatusError},
};

use crate::{
    channel::{ChannelError, CommandChannel},
    control::{ControlChannel, ControlError},
    keepalive::DatagramKeepalive,
    network::{Connection, Network},
    scan::{Clock, PageSink},
    search::{SearchError, search},
    session::{Host, JobFailure, PrepareError, Prepared, Reserved, SessionError, prepare, reserve},
};

/// Identifies this host in every frame.
/// The value is locally administered and fixed.
pub const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);

const DISCOVERY_PORT: u16 = 52217;
/// Gives a scanner in power save about ten seconds of half-second probe windows to wake up.
const SEARCH_ATTEMPTS: NonZeroU8 = NonZeroU8::MIN.saturating_add(19);
const STATUS_CAPACITY: usize = 64;
const REPLY_CAPACITY: usize = 0x0030_0040;

/// Why the scanner couldn't be found or its connections couldn't open.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenFailure<E> {
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
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failure<E, K> {
    Open(OpenFailure<E>),
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

/// The outcome of a job on a reserved scanner and the first failure of its keepalive port, which never stops the job.
#[derive(Debug, PartialEq, Eq)]
pub struct JobRun<E, K> {
    pub outcome: Result<Scanned, JobFailure<E, K>>,
    pub keepalive_failure: Option<E>,
}

struct Found<D> {
    reply: SearchReply,
    local: Ipv4Address,
    discovery: D,
}

fn find<N: Network>(
    network: &mut N,
    target: Ipv4Addr,
) -> Result<Found<N::Discovery>, OpenFailure<N::Error>> {
    let local = network
        .local_address(target)
        .map_err(|error| OpenFailure::Route { target, error })?;
    let mut discovery = network.open_discovery().map_err(OpenFailure::Discovery)?;
    let reply = search(
        &mut discovery,
        Probe::search(local, HOST_MAC),
        SocketAddrV4::new(target, DISCOVERY_PORT),
        SEARCH_ATTEMPTS,
    )
    .map_err(|error| match error {
        SearchError::Port(error) => OpenFailure::Discovery(error),
        SearchError::NoReply { attempts } => OpenFailure::NoReply { target, attempts },
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

fn connect<N: Network>(
    network: &mut N,
    endpoint: SocketAddrV4,
    connection: Connection,
) -> Result<N::Stream, OpenFailure<N::Error>> {
    network
        .connect(endpoint, connection)
        .map_err(|error| OpenFailure::Connect { connection, error })
}

fn open_command<N: Network>(
    network: &mut N,
    reply: &SearchReply,
    local: Ipv4Address,
) -> Result<CommandChannel<N::Stream>, OpenFailure<N::Error>> {
    let endpoint = scanner(reply, reply.command_port().number());
    let stream = connect(network, endpoint, Connection::Command)?;
    CommandChannel::open(stream, HOST_MAC, local).map_err(OpenFailure::Command)
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
    find(network, target)
        .map(|found| found.reply)
        .map_err(Failure::Open)
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
    let found = find(network, target).map_err(Failure::Open)?;
    let mut command = open_command(network, &found.reply, found.local).map_err(Failure::Open)?;
    let mut buffer = [0; STATUS_CAPACITY];
    let answer = command
        .execute(&Command::hardware_status(), &mut buffer)
        .map_err(|error| Failure::Open(OpenFailure::Command(error)))?;
    if answer.status != ScsiStatus::Good {
        return Err(Failure::StatusRefused(answer.status));
    }
    let status = HardwareStatus::decode(answer.data).map_err(Failure::Status)?;
    Ok(State {
        reply: found.reply,
        status,
    })
}

/// A scanner found on the network with both connections open and no reserve.
pub struct Connected<N: Network> {
    host: Host,
    command: CommandChannel<N::Stream>,
    control: ControlChannel<N::Stream>,
    clock: N::Clock,
    keepalive: DatagramKeepalive<N::Keepalive>,
    /// Keeps the port for button events bound while the scanner may send them.
    _trigger: N::Trigger,
}

/// Finds the scanner at `target`, which may be the broadcast address, and opens its connections without a reserve.
///
/// # Errors
///
/// Fails when the network fails, no scanner answers, or a connection doesn't open.
pub fn open<N: Network>(
    network: &mut N,
    target: Ipv4Addr,
) -> Result<Connected<N>, OpenFailure<N::Error>> {
    let found = find(network, target)?;
    let (trigger, trigger_port) = network
        .open_trigger(found.local)
        .map_err(OpenFailure::Trigger)?;
    let time = network.local_time().map_err(OpenFailure::Clock)?;
    let command = open_command(network, &found.reply, found.local)?;
    let control_endpoint = scanner(&found.reply, found.reply.control_port().number());
    let control_stream = connect(network, control_endpoint, Connection::Control)?;
    let control = ControlChannel::open(control_stream).map_err(OpenFailure::Control)?;
    let mut clock = network.clock();
    let keepalive_port = network
        .keepalive(found.discovery)
        .map_err(OpenFailure::Keepalive)?;
    let keepalive = DatagramKeepalive::new(
        keepalive_port,
        Probe::keepalive(found.local, HOST_MAC),
        scanner(&found.reply, DISCOVERY_PORT),
        clock.now(),
    );
    Ok(Connected {
        host: Host {
            mac: HOST_MAC,
            address: found.local,
            trigger_port,
            time,
        },
        command,
        control,
        clock,
        keepalive,
        _trigger: trigger,
    })
}

impl<N: Network> Connected<N> {
    /// Reads the registered host identifier and the paper position without a reserve.
    ///
    /// # Errors
    ///
    /// Fails when the status read fails, is refused, or is malformed, or when no host is registered.
    pub fn prepare(&mut self) -> Result<Prepared, PrepareError<N::Error>> {
        prepare(&mut self.command)
    }

    /// Reserves the scanner with the identifier that `prepare` read.
    ///
    /// # Errors
    ///
    /// Fails when the control channel fails or the scanner refuses the reserve.
    pub fn reserve(
        mut self,
        prepared: Prepared,
    ) -> Result<ReservedScanner<N>, ControlError<N::Error>> {
        let reserved = reserve(&mut self.control, self.host, prepared)?;
        Ok(ReservedScanner {
            connected: self,
            reserved,
        })
    }
}

/// A scanner reserved for this host until `scan` releases it.
#[must_use = "the scanner stays reserved until `scan` releases it"]
pub struct ReservedScanner<N: Network> {
    connected: Connected<N>,
    reserved: Reserved,
}

impl<N: Network> ReservedScanner<N> {
    /// Scans the stack in the top chute into the sink, keeps the reserve alive meanwhile, and releases the scanner after the job.
    ///
    /// The run carries the outcome and any failure of the keepalive port, which never stops the job.
    pub fn scan<K: PageSink>(self, sink: &mut K) -> JobRun<N::Error, K::Error> {
        let Self {
            mut connected,
            reserved,
        } = self;
        let mut buffer = vec![0_u8; REPLY_CAPACITY];
        let outcome = reserved.scan(
            &mut connected.command,
            &mut connected.control,
            &mut connected.keepalive,
            &mut connected.clock,
            sink,
            &mut buffer,
        );
        let keepalive_replies = connected.keepalive.replies();
        let (_, keepalive_failure) = connected.keepalive.into_parts();
        JobRun {
            outcome: outcome.map(|summary| Scanned {
                sheets: summary.sheets,
                keepalive_replies,
            }),
            keepalive_failure,
        }
    }
}

fn reserve_scanner<N: Network, K>(
    network: &mut N,
    target: Ipv4Addr,
) -> Result<ReservedScanner<N>, Failure<N::Error, K>> {
    let mut connected = open(network, target).map_err(Failure::Open)?;
    let prepared = connected
        .prepare()
        .map_err(|failure| Failure::Session(SessionError::Prepare(failure)))?;
    connected
        .reserve(prepared)
        .map_err(|failure| Failure::Session(SessionError::Reserve(failure)))
}

/// Finds the scanner, scans the stack in its top chute into the sink, and keeps the reserve alive meanwhile.
///
/// The run carries the outcome and any failure of the keepalive port, which never stops the job.
pub fn scan<N: Network, K: PageSink>(
    network: &mut N,
    target: Ipv4Addr,
    sink: &mut K,
) -> ScanRun<N::Error, K::Error> {
    match reserve_scanner(network, target) {
        Ok(reserved) => {
            let run = reserved.scan(sink);
            ScanRun {
                outcome: run
                    .outcome
                    .map_err(|failure| Failure::Session(SessionError::Job(failure))),
                keepalive_failure: run.keepalive_failure,
            }
        }
        Err(failure) => ScanRun {
            outcome: Err(failure),
            keepalive_failure: None,
        },
    }
}
