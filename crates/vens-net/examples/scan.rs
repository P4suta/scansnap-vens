//! Scans every sheet in the top chute once and writes one image file for each side.
//! Usage: `scan <output-directory> [scanner-address [trace-file]]`.
//! Load one sheet: the scanner feeds the whole stack, and this command sends no keepalive for a long job.
//! A trace file receives every frame of the run with its identifiers and image bytes, so keep it out of version control.

use std::{
    env, fmt, fs,
    io::{self, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use vens_core::{Ipv4Address, MacAddress, Port, control::LocalTime, discovery::Probe};
use vens_net::{
    channel::{ChannelError, CommandChannel},
    control::{ControlChannel, ControlError},
    host::{DirectorySink, SinkError, SleepingClock, TcpPort, UdpPort, local_address_toward},
    keepalive::DatagramKeepalive,
    record::Recorder,
    scan::Clock,
    search::{SearchError, search},
    session::{Host, SessionError, scan_once},
    trace::{self, Channel, Event},
};

const DISCOVERY_PORT: u16 = 52217;
const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const ROUTE_PROBE: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);
const REPLY_CAPACITY: usize = 0x0030_0040;

enum Failure {
    Usage,
    Address(String),
    Io(io::Error),
    Clock,
    Search(SearchError<io::Error>),
    Command(ChannelError<io::Error>),
    Control(ControlError<io::Error>),
    Session(SessionError<io::Error, SinkError>),
    Trace(io::Error),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str(
                "usage: scan <output-directory> [scanner-address [trace-file]]; load one sheet only",
            ),
            Self::Address(text) => write!(formatter, "`{text}` is not an IPv4 address"),
            Self::Io(error) => write!(formatter, "input or output failure: {error}"),
            Self::Clock => formatter.write_str("the system clock is outside the supported range"),
            Self::Search(SearchError::Port(error)) => {
                write!(formatter, "discovery socket failure: {error}")
            }
            Self::Search(SearchError::NoReply { attempts }) => {
                write!(formatter, "no scanner answered {attempts} probes")
            }
            Self::Command(error) => write!(formatter, "command channel failure: {error:?}"),
            Self::Control(error) => write!(formatter, "control channel failure: {error:?}"),
            Self::Session(error) => write!(formatter, "scan session failure: {error:?}"),
            Self::Trace(error) => write!(formatter, "trace file failure: {error}"),
        }
    }
}

struct Arguments {
    directory: PathBuf,
    target: Ipv4Addr,
    trace: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, Failure> {
    let mut arguments = env::args().skip(1);
    let directory = arguments.next().map(PathBuf::from).ok_or(Failure::Usage)?;
    let target = arguments.next().map_or(Ok(Ipv4Addr::BROADCAST), |text| {
        text.parse().map_err(|_| Failure::Address(text))
    })?;
    let trace = arguments.next().map(PathBuf::from);
    Ok(Arguments {
        directory,
        target,
        trace,
    })
}

fn now() -> Result<LocalTime, Failure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| LocalTime::from_unix_seconds(elapsed.as_secs()))
        .ok_or(Failure::Clock)
}

fn trigger_port(socket: &UdpSocket) -> Result<Port, Failure> {
    match socket.local_addr().map_err(Failure::Io)? {
        SocketAddr::V4(address) => Ok(Port::new(address.port())),
        SocketAddr::V6(_) => Err(Failure::Io(io::Error::other(
            "the trigger socket isn't IPv4",
        ))),
    }
}

fn scan(
    arguments: &Arguments,
    events: &mpsc::Sender<Event>,
    out: &mut impl Write,
) -> Result<(), Failure> {
    fs::create_dir_all(&arguments.directory).map_err(Failure::Io)?;
    let target = arguments.target;
    let route = if target.is_broadcast() {
        ROUTE_PROBE
    } else {
        target
    };
    let local = local_address_toward(route).map_err(Failure::Io)?;
    let address = Ipv4Address::new(local.octets());
    let port = UdpPort::open(PROBE_INTERVAL).map_err(Failure::Io)?;
    let mut discovery = Recorder::new(port, Channel::Discovery, events.clone());
    let scanner = search(
        &mut discovery,
        Probe::search(address, HOST_MAC),
        SocketAddrV4::new(target, DISCOVERY_PORT),
    )
    .map_err(Failure::Search)?;
    let scanner_address = Ipv4Addr::from(scanner.address().octets());
    let triggers = UdpSocket::bind((local, 0)).map_err(Failure::Io)?;
    let host = Host {
        mac: HOST_MAC,
        address,
        trigger_port: trigger_port(&triggers)?,
        time: now()?,
    };
    let command_port = SocketAddrV4::new(scanner_address, scanner.command_port().number());
    let control_port = SocketAddrV4::new(scanner_address, scanner.control_port().number());
    let mut command = TcpPort::connect(command_port, COMMAND_DEADLINE)
        .map_err(Failure::Io)
        .and_then(|stream| {
            let stream = Recorder::new(stream, Channel::Command, events.clone());
            CommandChannel::open(stream, HOST_MAC, address).map_err(Failure::Command)
        })?;
    let mut control = TcpPort::connect(control_port, COMMAND_DEADLINE)
        .map_err(Failure::Io)
        .and_then(|stream| {
            let stream = Recorder::new(stream, Channel::Control, events.clone());
            ControlChannel::open(stream).map_err(Failure::Control)
        })?;
    let mut clock = SleepingClock::new();
    let keepalive_port = discovery
        .into_port()
        .into_nonblocking()
        .map_err(Failure::Io)?;
    let mut keepalive = DatagramKeepalive::new(
        Recorder::new(keepalive_port, Channel::Discovery, events.clone()),
        Probe::keepalive(address, HOST_MAC),
        SocketAddrV4::new(scanner_address, DISCOVERY_PORT),
        clock.now(),
    );
    let mut sink = DirectorySink::new(arguments.directory.clone());
    let mut buffer = vec![0_u8; REPLY_CAPACITY];
    let outcome = scan_once(
        &mut command,
        &mut control,
        &mut keepalive,
        host,
        &mut clock,
        &mut sink,
        &mut buffer,
    );
    writeln!(out, "keepalive replies: {}", keepalive.replies()).map_err(Failure::Io)?;
    if let (_, Some(error)) = keepalive.into_parts() {
        writeln!(out, "keepalive failure: {error}").map_err(Failure::Io)?;
    }
    let summary = outcome.map_err(Failure::Session)?;
    writeln!(out, "sheets: {}", summary.sheets).map_err(Failure::Io)?;
    sink.written()
        .iter()
        .try_for_each(|path| writeln!(out, "{}", path.display()))
        .map_err(Failure::Io)
}

fn save_trace(path: &Path, log: &mpsc::Receiver<Event>) -> Result<(), Failure> {
    let events: Vec<Event> = log.try_iter().collect();
    fs::write(path, trace::render(&events)).map_err(Failure::Trace)
}

fn run(out: &mut impl Write) -> Vec<Failure> {
    let arguments = match arguments() {
        Ok(arguments) => arguments,
        Err(failure) => return Vec::from([failure]),
    };
    let (events, log) = mpsc::channel();
    let scanned = scan(&arguments, &events, out).err();
    let saved = arguments
        .trace
        .as_deref()
        .and_then(|path| save_trace(path, &log).err());
    scanned.into_iter().chain(saved).collect()
}

fn main() -> ExitCode {
    let failures = run(&mut io::stdout().lock());
    let mut errors = io::stderr().lock();
    for failure in &failures {
        let _ = writeln!(errors, "scan failed: {failure}");
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
